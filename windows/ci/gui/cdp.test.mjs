// cdp.mjs 시험 — 순수 함수 표와, 손으로 만든 가짜 CDP 서버(http + WebSocket 핸드셰이크)에 실제로 붙어 보는 시험.
// 실행: node --test windows/ci/gui/cdp.test.mjs   (보드·WebView2 를 띄우지 않는다)
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { connect, normalizeRect, parseArgs, parseRect, pickTarget, checkedOutput } from './cdp.mjs';
import { boardTargets, serveFakeBoard as fakeBoard } from './testkit/fakeCdpServer.mjs';

const PNG_BYTES = Buffer.from('89504e470d0a1a0a0000000d49484452', 'hex');

// ── 순수 함수 ───────────────────────────────────────────────────────────────

test('parseRect — 숫자 넷만 받는다', () => {
  assert.deepEqual(parseRect('1,2,30,40'), { x: 1, y: 2, width: 30, height: 40 });
  assert.deepEqual(parseRect(' 0 , 0 , 5.5 , 6 '), { x: 0, y: 0, width: 5.5, height: 6 });
  for (const bad of ['', '1,2,3', '1,2,3,4,5', 'a,b,c,d', '1,2,0,4', '1,2,3,-4', '1,2,3,']) assert.throws(() => parseRect(bad), undefined, bad);
});

test('normalizeRect — 뷰포트 안쪽으로 자르고 밖이면 던진다', () => {
  const view = { width: 100, height: 50 };
  assert.deepEqual(normalizeRect({ x: 10, y: 5, width: 20, height: 10 }, view), { x: 10, y: 5, width: 20, height: 10 });
  assert.deepEqual(normalizeRect({ x: -10, y: -5, width: 30, height: 20 }, view), { x: 0, y: 0, width: 20, height: 15 });
  assert.deepEqual(normalizeRect({ x: 90, y: 40, width: 50, height: 50 }, view), { x: 90, y: 40, width: 10, height: 10 });
  assert.throws(() => normalizeRect({ x: 100, y: 0, width: 10, height: 10 }, view), /창 밖/);
  assert.throws(() => normalizeRect({ x: 0, y: 50, width: 10, height: 10 }, view), /창 밖/);
  assert.throws(() => normalizeRect({ x: 0, y: 0, width: NaN, height: 10 }, view), /수가 아니다/);
  assert.throws(() => normalizeRect(null), /수가 아니다/);
  assert.deepEqual(normalizeRect({ x: 1, y: 2, width: 3, height: 4 }), { x: 1, y: 2, width: 3, height: 4 });
});

test('pickTarget — 제목이 "Claude Board" 인 페이지(기본), 주소 조각이면 그것', () => {
  const targets = [
    { type: 'page', title: 'Notify', url: 'http://tauri.localhost/notify', webSocketDebuggerUrl: 'ws://127.0.0.1:1/a' },
    { type: 'background_page', title: 'Claude Board', url: 'x', webSocketDebuggerUrl: 'ws://127.0.0.1:1/b' },
    { type: 'page', title: 'Claude Board', url: 'http://tauri.localhost/', webSocketDebuggerUrl: 'ws://127.0.0.1:1/c' },
    { type: 'page', title: 'Claude Board', url: 'http://tauri.localhost/', webSocketDebuggerUrl: undefined },
  ];
  assert.equal(pickTarget(targets).webSocketDebuggerUrl, 'ws://127.0.0.1:1/c');
  assert.equal(pickTarget(targets, { urlIncludes: '/notify' }).title, 'Notify');
  assert.equal(pickTarget(targets, { title: '없는 제목' }), null);
  assert.equal(pickTarget([]), null);
});

test('parseArgs — 값이 있는 옵션과 위치 인자, 모르는 옵션은 던진다', () => {
  assert.deepEqual(parseArgs(['--port', '9333', 'eval', '1+1']), { flags: { port: '9333' }, rest: ['eval', '1+1'] });
  assert.deepEqual(parseArgs(['shot', 'a.png', '--rect', '1,2,3,4']), { flags: { rect: '1,2,3,4' }, rest: ['shot', 'a.png'] });
  assert.throws(() => parseArgs(['--port']), /값이 없다/);
  assert.throws(() => parseArgs(['--remote', 'x']), /알 수 없는/);
});

test('checkedOutput — <repo>/e2e/{out,artifacts}/ 아래 .png 만, 그 밖과 회사 경로는 거절', () => {
  const repo = fs.mkdtempSync(path.join(os.tmpdir(), 'cdp-repo-'));
  try {
    const ok = checkedOutput(path.join(repo, 'e2e', 'out', 'a', 'b.png'), repo);
    assert.ok(ok.endsWith('b.png'));
    assert.ok(fs.existsSync(path.join(repo, 'e2e', 'out', 'a')));
    for (const bad of [path.join(repo, 'x.png'), path.join(repo, 'e2e', 'x.png'), path.join(repo, 'e2e', 'other', 'x.png'), path.join(repo, 'e2e', 'out', 'x.jpg'), path.join(repo, 'e2e', 'out', '..', 'x.png'), path.join(repo, 'e2e', 'out', 'aijinet-project', 'x.png')]) {
      assert.throws(() => checkedOutput(bad, repo), undefined, bad);
    }
    assert.equal(fs.existsSync(path.join(repo, 'e2e', 'other')), false, '거절한 경로의 폴더를 만들지 않는다');
  } finally {
    fs.rmSync(repo, { recursive: true, force: true });
  }
});


test('evaluate — Runtime.evaluate 로 값을 받고 JS 예외는 던진다', async () => {
  const board = await fakeBoard({
    targets: boardTargets,
    handler: (m) => {
      if (m.params.expression === '1+1') return { result: { result: { type: 'number', value: 2 } } };
      if (m.params.expression === 'boom') return { result: { exceptionDetails: { text: 'Uncaught', exception: { description: 'Error: boom' } } } };
      return { error: { message: 'unexpected' } };
    },
  });
  const page = await connect({ port: board.port });
  try {
    assert.equal(page.target.title, 'Claude Board');
    assert.equal(await page.evaluate('1+1'), 2);
    await assert.rejects(page.evaluate('boom'), /JS 예외: Error: boom/);
    await assert.rejects(page.evaluate('other'), /unexpected/);
    assert.deepEqual(board.calls[0].params, { expression: '1+1', awaitPromise: true, returnByValue: true });
  } finally {
    page.close();
    await board.close();
  }
});

test('cropTo — 뷰포트 안쪽으로 자른 clip 으로 캡처해 e2e/out 에 쓴다', async () => {
  const repo = fs.mkdtempSync(path.join(os.tmpdir(), 'cdp-crop-'));
  const board = await fakeBoard({
    targets: boardTargets,
    handler: (m) => {
      if (m.method === 'Runtime.evaluate') return { result: { result: { value: { width: 800, height: 600 } } } };
      if (m.method === 'Page.captureScreenshot') return { result: { data: PNG_BYTES.toString('base64') } };
      return { error: { message: 'unexpected ' + m.method } };
    },
  });
  const page = await connect({ port: board.port });
  try {
    const out = path.join(repo, 'e2e', 'out', 'shot.png');
    const written = await page.cropTo({ x: -5, y: 10, width: 100, height: 700 }, { output: out, repo });
    assert.equal(written.bytes, PNG_BYTES.length);
    assert.deepEqual(fs.readFileSync(out), PNG_BYTES);
    const shot = board.calls.find((m) => m.method === 'Page.captureScreenshot');
    assert.deepEqual(shot.params.clip, { x: 0, y: 10, width: 95, height: 590, scale: 1 });
    assert.equal(shot.params.captureBeyondViewport, false);
    // e2e 밖에는 쓰지 않는다
    await assert.rejects(page.cropTo({ x: 0, y: 0, width: 10, height: 10 }, { output: path.join(repo, 'loose.png'), repo }), /e2e/);
    assert.equal(fs.existsSync(path.join(repo, 'loose.png')), false);
    // 창 밖 사각형은 캡처를 보내기 전에 거절한다
    const before = board.calls.length;
    await assert.rejects(page.cropTo({ x: 900, y: 0, width: 10, height: 10 }, { output: out, repo }), /창 밖/);
    assert.equal(board.calls.filter((m) => m.method === 'Page.captureScreenshot').length, 1);
    assert.ok(board.calls.length > before, '뷰포트 크기만 물었다');
  } finally {
    page.close();
    await board.close();
    fs.rmSync(repo, { recursive: true, force: true });
  }
});

test('rectOf — 선택자를 JSON 문자열로 안전하게 넣어 묻는다', async () => {
  const board = await fakeBoard({ targets: boardTargets, handler: () => ({ result: { result: { value: { x: 1, y: 2, width: 3, height: 4 } } } }) });
  const page = await connect({ port: board.port });
  try {
    assert.deepEqual(await page.rectOf('[data-testid="a\'b"]'), { x: 1, y: 2, width: 3, height: 4 });
    assert.ok(board.calls[0].params.expression.includes(JSON.stringify('[data-testid="a\'b"]')));
  } finally {
    page.close();
    await board.close();
  }
});

test('연결 거절 — 로컬이 아닌 호스트, 포트 없음, 붙을 페이지 없음, 웹소켓 주소가 로컬이 아님', async () => {
  await assert.rejects(connect({ port: 9, host: 'example.com' }), /로컬 보드에만/);
  await assert.rejects(connect({}), /포트가 필요/);
  await assert.rejects(connect({ port: 'abc' }), /포트가 필요/);
  const empty = await fakeBoard({ targets: () => [], handler: () => undefined });
  const remote = await fakeBoard({ targets: () => [{ type: 'page', title: 'Claude Board', url: 'x', webSocketDebuggerUrl: 'ws://example.com:1/x' }], handler: () => undefined });
  try {
    await assert.rejects(connect({ port: empty.port }), /붙을 페이지가 없다/);
    await assert.rejects(connect({ port: remote.port }), /웹소켓 주소가 로컬이 아니다/);
  } finally {
    await empty.close();
    await remote.close();
  }
});

test('답이 없으면 시간 초과로 던진다', async () => {
  const board = await fakeBoard({ targets: boardTargets, handler: () => undefined });
  const page = await connect({ port: board.port, timeoutMs: 200 });
  try {
    await assert.rejects(page.evaluate('1'), /200ms 안에 답이 없다/);
  } finally {
    page.close();
    await board.close();
  }
});
