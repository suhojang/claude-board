'use strict';
// fake-claude-stream.cjs 자체 시험 — 파이프로 띄운 자식에 user 줄을 써 넣고 stream-json 출력·종료 코드·기록 파일을 확인한다.
// 실행: node --test windows/ci/fake-claude-stream.test.cjs
const { test } = require('node:test');
const assert = require('node:assert/strict');
const cp = require('node:child_process');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const SCRIPT = path.join(__dirname, 'fake-claude-stream.cjs');
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;

async function waitFor(predicate, what, timeoutMs = 8000) {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    const value = predicate();
    if (value) return value;
    if (Date.now() > deadline) throw new Error(`시간 초과: ${what}`);
    await sleep(20);
  }
}

// 설계.md 4장 「한도 거절 가리기」의 세 갈래를 그대로 옮긴 판별 — 가짜가 낸 줄이 각각 어느 갈래에 걸리는지 본다
const isRateLimitEventRejected = (e) => e.type === 'rate_limit_event' && e.rate_limit_info && e.rate_limit_info.status === 'rejected';
const isAssistantRateLimit = (e) => e.type === 'assistant' && (e.error === 'rate_limit' || e.api_error_status === 429);
const isResultRateLimit = (e) => e.type === 'result' && e.is_error === true && (e.api_error_status === 429 || e.api_error === 'usage_limit_reached');
const shapes = (events) => ({
  a: events.some(isRateLimitEventRejected),
  b: events.some(isAssistantRateLimit),
  c: events.some(isResultRateLimit),
});

function userLine(text) {
  return JSON.stringify({ type: 'user', message: { role: 'user', content: text } }) + '\n';
}

function interruptLine(id = 'req-1') {
  return JSON.stringify({ type: 'control_request', request_id: id, request: { subtype: 'interrupt' } }) + '\n';
}

// script: 턴 배열(있으면 파일로 쓴다). 작업 폴더(cwd)는 임시 폴더라 가짜의 쓰기는 그 안에만 닿는다
function start({ args = ['--session-id', '11111111-2222-4333-8444-555555555555'], script = null, env = {} } = {}) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'fake-claude-stream-'));
  const cwd = path.join(dir, 'work');
  fs.mkdirSync(cwd);
  const log = path.join(dir, 'fake.log');
  const base = Object.assign({}, process.env, { FAKE_CLAUDE_LOG: log, FAKE_CLAUDE_MAX_MS: '30000' });
  for (const key of ['FAKE_CLAUDE_SCRIPT', 'FAKE_CLAUDE_STATE']) delete base[key];
  if (script) {
    const scriptFile = path.join(dir, 'script.json');
    fs.writeFileSync(scriptFile, JSON.stringify(script));
    base.FAKE_CLAUDE_SCRIPT = scriptFile;
  }
  return { dir, cwd, log, env: Object.assign(base, env), args };
}

// 같은 start() 결과로 여러 번 띄울 수 있다(엔진이 턴마다 프로세스를 새로 띄우는 것처럼)
function spawnFake(setup, args = setup.args) {
  const child = cp.spawn(process.execPath, [SCRIPT, ...args], { cwd: setup.cwd, stdio: ['pipe', 'pipe', 'pipe'], env: setup.env });
  let buffer = '';
  const events = [];
  child.stdout.on('data', (d) => {
    buffer += d.toString('utf8');
    let at;
    while ((at = buffer.indexOf('\n')) >= 0) {
      const line = buffer.slice(0, at);
      buffer = buffer.slice(at + 1);
      if (line.trim()) events.push(JSON.parse(line));
    }
  });
  child.stderr.on('data', () => {});
  const exited = new Promise((resolve) => child.on('exit', (code) => resolve(code)));
  return { child, events, exited, send: (text) => child.stdin.write(text) };
}

const rows = (setup) => (fs.existsSync(setup.log) ? fs.readFileSync(setup.log, 'utf8').split('\n').filter(Boolean).map((row) => JSON.parse(row)) : []);
const cleanup = (setup) => fs.rmSync(setup.dir, { recursive: true, force: true, maxRetries: 20, retryDelay: 50 });
const results = (events) => events.filter((e) => e.type === 'result');

// 한 턴을 보내고 입력을 닫아 끝나기를 기다린다
async function oneTurn(setup, args, text = 'hello') {
  const run = spawnFake(setup, args);
  run.send(userLine(text));
  run.child.stdin.end();
  const code = await run.exited;
  return { code, events: run.events };
}

test('--version — 버전 한 줄만 내고 입력을 기다리지 않고 0 으로 끝난다', () => {
  const r = cp.spawnSync(process.execPath, [SCRIPT, '--version'], { encoding: 'utf8', input: '', timeout: 5000 });
  assert.equal(r.status, 0);
  assert.match(r.stdout, /^\d+\.\d+\.\d+ \(Claude Code\)\r?\n$/);
});

test('새 세션 — init 에 session_id, 사용률 사건, assistant 글, result 의 total_cost_usd·num_turns, 입력이 닫히면 0', async () => {
  const setup = start();
  try {
    const { code, events } = await oneTurn(setup);
    assert.equal(code, 0);
    assert.deepEqual(events.map((e) => e.type), ['system', 'rate_limit_event', 'assistant', 'result']);
    const [init, limit, assistant, result] = events;
    assert.equal(init.subtype, 'init');
    assert.equal(init.session_id, '11111111-2222-4333-8444-555555555555');
    assert.equal(limit.rate_limit_info.status, 'allowed');
    assert.equal(limit.rate_limit_info.unifiedWindows.five_hour.utilization, 0.03);
    assert.equal(limit.rate_limit_info.unifiedWindows.seven_day.utilization, 0.1);
    assert.ok(limit.rate_limit_info.unifiedWindows.seven_day.resetsAt > Date.now() / 1000);
    assert.match(assistant.message.content[0].text, /hello/);
    assert.equal(result.subtype, 'success');
    assert.equal(result.is_error, false);
    assert.equal(result.total_cost_usd, 0.01);
    assert.equal(result.num_turns, 1);
    assert.equal(result.session_id, init.session_id);
    assert.deepEqual(shapes(events), { a: false, b: false, c: false });
  } finally {
    cleanup(setup);
  }
});

test('--resume — 받은 id 가 init·result 의 session_id 가 되고 기록에 resume 으로 남는다', async () => {
  const id = 'aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee';
  const setup = start({ args: ['--resume', id] });
  try {
    const { code, events } = await oneTurn(setup);
    assert.equal(code, 0);
    assert.equal(events[0].session_id, id);
    assert.equal(results(events)[0].session_id, id);
    const started = rows(setup).find((r) => r.event === 'start');
    assert.equal(started.resume, true);
    assert.equal(started.sessionId, id);
  } finally {
    cleanup(setup);
  }
});

test('세션 id 인자가 없으면 새 uuid 를 만든다', async () => {
  const setup = start({ args: ['--model', 'sonnet'] });
  try {
    const { events } = await oneTurn(setup);
    assert.match(events[0].session_id, UUID);
  } finally {
    cleanup(setup);
  }
});

test('한 프로세스에 user 줄을 이어 보내면 턴마다 result 가 하나씩 나온다', async () => {
  const setup = start({ script: [{ cost: 0.5 }, { cost: 0.25 }] });
  try {
    const run = spawnFake(setup);
    run.send(userLine('one'));
    await waitFor(() => results(run.events).length === 1, '첫 result');
    run.send(userLine('two'));
    run.child.stdin.end();
    assert.equal(await run.exited, 0);
    assert.deepEqual(results(run.events).map((r) => r.total_cost_usd), [0.5, 0.25]);
    assert.equal(run.events.filter((e) => e.subtype === 'init').length, 1);
  } finally {
    cleanup(setup);
  }
});

test('user 줄의 content 가 블록 배열이어도 글을 읽는다', async () => {
  const setup = start();
  try {
    const run = spawnFake(setup);
    run.send(JSON.stringify({ type: 'user', message: { role: 'user', content: [{ type: 'text', text: 'blocks ' }, { type: 'text', text: 'ok' }] } }) + '\n');
    run.child.stdin.end();
    await run.exited;
    assert.match(run.events.find((e) => e.type === 'assistant').message.content[0].text, /blocks ok/);
    assert.ok(rows(setup).some((r) => r.event === 'turn' && r.userText === 'blocks ok'));
  } finally {
    cleanup(setup);
  }
});

test('스크립트 쓰기 — tool_use(Write) → 작업 폴더 안 파일 → tool_result, num_turns 가 쓰기 수만큼 늘어난다', async () => {
  const setup = start({ script: [{ writes: [{ path: 'sub/out.txt', content: 'hi\n' }, { path: 'b.txt', content: 'bee' }], cost: 0.07 }] });
  try {
    const { code, events } = await oneTurn(setup);
    assert.equal(code, 0);
    assert.equal(fs.readFileSync(path.join(setup.cwd, 'sub', 'out.txt'), 'utf8'), 'hi\n');
    assert.equal(fs.readFileSync(path.join(setup.cwd, 'b.txt'), 'utf8'), 'bee');
    const toolUses = events.filter((e) => e.type === 'assistant' && e.message.content[0].type === 'tool_use');
    assert.equal(toolUses.length, 2);
    assert.equal(toolUses[0].message.content[0].name, 'Write');
    assert.equal(toolUses[0].message.content[0].input.file_path, 'sub/out.txt');
    const toolResults = events.filter((e) => e.type === 'user').map((e) => e.message.content[0]);
    assert.equal(toolResults.length, 2);
    assert.ok(toolResults.every((r) => r.type === 'tool_result' && r.is_error === false));
    assert.equal(toolResults[0].tool_use_id, toolUses[0].message.content[0].id);
    const result = results(events)[0];
    assert.equal(result.num_turns, 3);
    assert.equal(result.total_cost_usd, 0.07);
  } finally {
    cleanup(setup);
  }
});

test('작업 폴더 밖 쓰기는 하지 않고 tool_result 가 오류다(상대 ..·절대 경로)', async () => {
  const outside = path.join(os.tmpdir(), `fake-claude-stream-outside-${process.pid}.txt`);
  const setup = start({ script: [{ writes: [{ path: '../escape.txt', content: 'x' }, { path: outside, content: 'x' }] }] });
  try {
    const { code, events } = await oneTurn(setup);
    assert.equal(code, 0);
    assert.equal(fs.existsSync(path.join(setup.dir, 'escape.txt')), false);
    assert.equal(fs.existsSync(outside), false);
    const toolResults = events.filter((e) => e.type === 'user').map((e) => e.message.content[0]);
    assert.ok(toolResults.length === 2 && toolResults.every((r) => r.is_error === true));
    assert.equal(rows(setup).filter((r) => r.event === 'out-of-cwd').length, 2);
  } finally {
    cleanup(setup);
    fs.rmSync(outside, { force: true });
  }
});

test('턴 번호는 프로세스를 넘어 이어진다 — 새 세션 다음 --resume 이 스크립트의 둘째 턴을 한다', async () => {
  const id = '99999999-8888-4777-8666-555555555555';
  const setup = start({ args: ['--session-id', id], script: [{ writes: [{ path: 'first.txt', content: '1' }] }, { writes: [{ path: 'second.txt', content: '2' }], cost: 0.2 }] });
  try {
    await oneTurn(setup);
    assert.equal(fs.existsSync(path.join(setup.cwd, 'first.txt')), true);
    assert.equal(fs.existsSync(path.join(setup.cwd, 'second.txt')), false);
    const second = await oneTurn(setup, ['--resume', id]);
    assert.equal(fs.existsSync(path.join(setup.cwd, 'second.txt')), true);
    assert.equal(results(second.events)[0].total_cost_usd, 0.2);
    // 스크립트가 모자라면 평범한 턴이다
    const third = await oneTurn(setup, ['--resume', id]);
    assert.equal(results(third.events)[0].total_cost_usd, 0.01);
  } finally {
    cleanup(setup);
  }
});

test('한도 거절 모양 A — rate_limit_event 의 rejected 만 걸린다', async () => {
  const setup = start({ script: [{ limit: 'A', resets_in_s: 120 }] });
  try {
    const { events } = await oneTurn(setup);
    assert.deepEqual(shapes(events), { a: true, b: false, c: false });
    const rejected = events.find(isRateLimitEventRejected);
    assert.equal(rejected.rate_limit_info.rateLimitType, 'seven_day');
    assert.equal(rejected.rate_limit_info.unifiedWindows.seven_day.utilization, 1);
    const left = rejected.rate_limit_info.unifiedWindows.seven_day.resetsAt - Date.now() / 1000;
    assert.ok(left > 100 && left <= 121, String(left));
    assert.equal(results(events)[0].is_error, false);
  } finally {
    cleanup(setup);
  }
});

test('한도 거절 모양 B — assistant 의 error rate_limit·429 만 걸린다', async () => {
  const setup = start({ script: [{ limit: 'B' }] });
  try {
    const { events } = await oneTurn(setup);
    assert.deepEqual(shapes(events), { a: false, b: true, c: false });
    const assistant = events.find(isAssistantRateLimit);
    assert.equal(assistant.error, 'rate_limit');
    assert.equal(assistant.api_error_status, 429);
    assert.equal(assistant.message.model, '<synthetic>');
    assert.equal(results(events)[0].is_error, false);
  } finally {
    cleanup(setup);
  }
});

test('한도 거절 모양 C — result 의 is_error·429·usage_limit_reached 만 걸리고 subtype 은 success', async () => {
  const setup = start({ script: [{ limit: 'C' }] });
  try {
    const { events } = await oneTurn(setup);
    assert.deepEqual(shapes(events), { a: false, b: false, c: true });
    const result = results(events)[0];
    assert.equal(result.subtype, 'success');
    assert.equal(result.is_error, true);
    assert.equal(result.api_error_status, 429);
    assert.equal(result.api_error, 'usage_limit_reached');
  } finally {
    cleanup(setup);
  }
});

test('한도 거절 모양 ABC — 실제 기록처럼 셋 다 걸린다', async () => {
  const setup = start({ script: [{ limit: 'ABC' }] });
  try {
    const { events } = await oneTurn(setup);
    assert.deepEqual(shapes(events), { a: true, b: true, c: true });
    assert.deepEqual(events.map((e) => e.type), ['system', 'rate_limit_event', 'assistant', 'result']);
  } finally {
    cleanup(setup);
  }
});

test('빈 result — num_turns 0·result "" 뿐이고 assistant 도 쓰기도 없다', async () => {
  const setup = start({ script: [{ empty: true, writes: [{ path: 'never.txt', content: 'x' }] }] });
  try {
    const { code, events } = await oneTurn(setup);
    assert.equal(code, 0);
    assert.deepEqual(events.map((e) => e.type), ['system', 'result']);
    const result = results(events)[0];
    assert.equal(result.num_turns, 0);
    assert.equal(result.result, '');
    assert.equal(result.is_error, false);
    assert.equal(result.subtype, 'success');
    assert.equal(fs.existsSync(path.join(setup.cwd, 'never.txt')), false);
  } finally {
    cleanup(setup);
  }
});

test('멈춤 — control_request interrupt 가 쉬는 중인 턴을 깨우고 aborted_tools result 와 130 으로 끝난다', async () => {
  const setup = start({ script: [{ delay_ms: 20000, writes: [{ path: 'late.txt', content: 'x' }] }] });
  try {
    const run = spawnFake(setup);
    run.send(userLine('work'));
    await waitFor(() => run.events.some((e) => e.type === 'assistant'), '턴 시작');
    const sentAt = Date.now();
    run.send(interruptLine('req-77'));
    assert.equal(await run.exited, 130);
    assert.ok(Date.now() - sentAt < 5000, '일찍 끝나야 한다');
    const response = run.events.find((e) => e.type === 'control_response');
    assert.deepEqual(response.response, { subtype: 'success', request_id: 'req-77' });
    const result = results(run.events)[0];
    assert.equal(result.subtype, 'error_during_execution');
    assert.equal(result.is_error, true);
    assert.equal(result.terminal_reason, 'aborted_tools');
    assert.equal(run.events[run.events.length - 1].type, 'result');
    assert.equal(fs.existsSync(path.join(setup.cwd, 'late.txt')), false);
  } finally {
    cleanup(setup);
  }
});

test('멈춤 — 입력을 기다리는 중에 받아도 130', async () => {
  const setup = start();
  try {
    const run = spawnFake(setup);
    run.send(userLine('quick'));
    await waitFor(() => results(run.events).length === 1, '첫 result');
    run.send(interruptLine());
    assert.equal(await run.exited, 130);
    assert.equal(results(run.events).length, 2);
    assert.equal(results(run.events)[1].terminal_reason, 'aborted_tools');
  } finally {
    cleanup(setup);
  }
});

test('exit_code — result 를 낸 뒤 입력이 열려 있어도 그 코드로 끝난다', async () => {
  const setup = start({ script: [{ exit_code: 3 }] });
  try {
    const run = spawnFake(setup);
    run.send(userLine('x'));
    assert.equal(await run.exited, 3);
    assert.equal(results(run.events).length, 1);
  } finally {
    cleanup(setup);
  }
});

test('crash — init 만 내고 result 없이 exit_code 로 끝난다', async () => {
  const setup = start({ script: [{ crash: true, exit_code: 9 }] });
  try {
    const run = spawnFake(setup);
    run.send(userLine('x'));
    assert.equal(await run.exited, 9);
    assert.deepEqual(run.events.map((e) => e.type), ['system']);
  } finally {
    cleanup(setup);
  }
});

test('argv 기록 — 모르는 인자까지 순서대로 남기고, --settings·시스템 프롬프트 파일 내용과 모델을 담는다', async () => {
  const setup = start();
  try {
    const settingsFile = path.join(setup.dir, 'settings.json');
    const promptFile = path.join(setup.dir, 'prompt.md');
    fs.writeFileSync(settingsFile, '{"permissions":{"deny":["Bash(git push:*)"]}}');
    fs.writeFileSync(promptFile, 'be careful');
    const args = [
      '-p', '--input-format', 'stream-json', '--output-format', 'stream-json', '--verbose',
      '--session-id', '11111111-2222-4333-8444-555555555555',
      '--settings', settingsFile, '--append-system-prompt-file', promptFile,
      '--model', 'sonnet', '--add-dir', setup.dir, '--plugin-dir', setup.dir,
      '--some-future-flag', 'value', '--permission-mode', 'acceptEdits',
    ];
    const { code } = await oneTurn(setup, args);
    assert.equal(code, 0);
    const started = rows(setup).find((r) => r.event === 'start');
    assert.deepEqual(started.argv, args);
    assert.equal(started.model, 'sonnet');
    assert.equal(started.settings, '{"permissions":{"deny":["Bash(git push:*)"]}}');
    assert.equal(started.systemPrompt, 'be careful');
    assert.equal(started.cwd, setup.cwd);
    assert.ok(rows(setup).some((r) => r.event === 'in' && JSON.parse(r.line).type === 'user'));
    assert.equal(rows(setup).at(-1).event, 'exit');
  } finally {
    cleanup(setup);
  }
});

test('읽을 수 없는 --settings 파일도 막지 않고 기록에 사유를 남긴다', async () => {
  const setup = start();
  try {
    const { code } = await oneTurn(setup, ['--session-id', '11111111-2222-4333-8444-555555555555', '--settings', path.join(setup.dir, 'missing.json')]);
    assert.equal(code, 0);
    assert.match(rows(setup).find((r) => r.event === 'start').settings, /^읽지 못함/);
  } finally {
    cleanup(setup);
  }
});

test('FAKE_CLAUDE_LOG 가 없으면 기록 파일을 만들지 않는다', async () => {
  const setup = start({ env: { FAKE_CLAUDE_LOG: '' } });
  try {
    const { code } = await oneTurn(setup);
    assert.equal(code, 0);
    assert.equal(fs.existsSync(setup.log), false);
  } finally {
    cleanup(setup);
  }
});
