// 격리한 보드의 WebView2 에 붙는 최소 CDP 도구 — 의존 패키지 없음(Node 24 내장 WebSocket·fetch).
// 보드는 launch-board.ps1 이 `--remote-debugging-port=<포트>` 로 띄운다. 이 도구는 그 포트의 페이지 하나에만 붙고, 127.0.0.1 밖으로는 나가지 않는다.
//
// 명령줄
//   node cdp.mjs --port <포트> targets                         페이지 목록(JSON)
//   node cdp.mjs --port <포트> eval '<JS 식>'                  결과를 JSON 으로 출력(Promise 는 기다린다)
//   node cdp.mjs --port <포트> shot <out.png> [--rect x,y,w,h] 화면 캡처. --rect 가 있으면 그 사각형(CSS 픽셀)만 — 창 밖은 담기지 않는다
//   포트는 --port 대신 환경변수 CDP_PORT 로도 줄 수 있다. 대상 고르기: --title <글>(기본 "Claude Board") 또는 --url <조각>
//   종료코드 0 성공, 1 실패(붙지 못함·JS 예외), 2 사용법
//
// 라이브러리
//   import { connect } from './cdp.mjs';  const page = await connect({ port });
//   page.evaluate(expr) · page.screenshot({ clip }) · page.rectOf(selector) · page.cropTo(rect, { output, repo }) · page.close()
//   cropTo 는 그 사각형만 캡처해 쓴다(전체 화면을 거치지 않는다) — Jev 에 보내는 그림은 이 길로 만든다.
//   `repo` 를 주면 출력이 <repo>/e2e/{out,artifacts}/ 아래일 때만 쓴다(Jev 가드와 같은 규칙).
//
// 판독하지 못한 것: CDP clip 사각형이 화면 배율(devicePixelRatio)이 1 이 아닐 때 출력 픽셀 수를 어떻게 정하는지는 이 PC(배율 100%)에서만 본다.
import fs from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { ALLOWED_SUBDIRS, GuardError, ensureNotCompany, ensureUnderE2e } from '../jev/domain/guard.mjs';

const LOCAL_HOSTS = new Set(['127.0.0.1', 'localhost']);

/** `x,y,너비,높이` 정수 넷 → {x,y,width,height}. 아니면 던진다 */
export function parseRect(text) {
  const parts = String(text).split(',').map((p) => p.trim());
  if (parts.length !== 4 || !parts.every((p) => /^-?\d+(\.\d+)?$/.test(p))) throw new Error(`사각형은 x,y,너비,높이 숫자 넷이다: ${text}`);
  const [x, y, width, height] = parts.map(Number);
  return normalizeRect({ x, y, width, height });
}

/** 사각형 검사 — 너비·높이는 양수, 좌표는 유한한 수. 뷰포트가 주어지면 안쪽으로 자른다(바깥은 보드 창이 아니다) */
export function normalizeRect(rect, viewport = null) {
  const { x, y, width, height } = rect ?? {};
  if (![x, y, width, height].every(Number.isFinite)) throw new Error(`사각형 값이 수가 아니다: ${JSON.stringify(rect)}`);
  if (width <= 0 || height <= 0) throw new Error(`사각형의 너비·높이는 양수여야 한다: ${JSON.stringify(rect)}`);
  if (!viewport) return { x, y, width, height };
  const left = Math.max(0, x);
  const top = Math.max(0, y);
  const right = Math.min(viewport.width, x + width);
  const bottom = Math.min(viewport.height, y + height);
  if (right <= left || bottom <= top) throw new Error(`사각형이 창 밖이다: ${JSON.stringify(rect)} (창 ${viewport.width}x${viewport.height})`);
  return { x: left, y: top, width: right - left, height: bottom - top };
}

/** /json 목록에서 붙을 페이지 — 제목(기본 "Claude Board")이나 주소 조각이 맞는 것. 여럿이면 처음 것 */
export function pickTarget(targets, { title = 'Claude Board', urlIncludes = '' } = {}) {
  const pages = targets.filter((t) => t.type === 'page' && t.webSocketDebuggerUrl);
  const wanted = pages.filter((t) => (urlIncludes ? String(t.url).includes(urlIncludes) : t.title === title));
  return wanted[0] ?? null;
}

/** 출력 위치 검사 — <repo>/e2e/{out,artifacts}/ 아래 .png 만. 폴더는 검사 뒤에 만들고 실제 경로로 다시 따진다 */
export function checkedOutput(output, repo) {
  ensureNotCompany(output, repo);
  const e2e = path.join(path.resolve(repo), 'e2e');
  const rel = path.relative(e2e, path.resolve(output));
  const parts = rel.split(path.sep);
  if (rel === '' || path.isAbsolute(rel) || parts[0] === '..' || parts.length < 2 || !ALLOWED_SUBDIRS.includes(parts[0])) {
    throw new GuardError(`그림은 ${path.join(e2e, `{${ALLOWED_SUBDIRS.join(',')}}`)} 아래에만 쓴다: ${output}`);
  }
  if (!output.toLowerCase().endsWith('.png')) throw new GuardError(`출력은 .png 여야 한다: ${output}`);
  fs.mkdirSync(path.dirname(path.resolve(output)), { recursive: true });
  return ensureUnderE2e(output, repo, { mustExist: false });
}

/** 페이지에 붙는다. host 는 로컬만 받는다 */
export async function connect({ port, host = '127.0.0.1', title, urlIncludes, timeoutMs = 15000 } = {}) {
  if (!Number.isInteger(Number(port)) || Number(port) <= 0) throw new Error('포트가 필요하다(--port 또는 CDP_PORT)');
  if (!LOCAL_HOSTS.has(host)) throw new Error(`로컬 보드에만 붙는다: ${host}`);
  const base = `http://${host}:${Number(port)}`;
  const targets = await (await fetch(`${base}/json`, { signal: AbortSignal.timeout(timeoutMs) })).json();
  const target = pickTarget(targets, { title, urlIncludes });
  if (!target) throw new Error(`붙을 페이지가 없다: ${targets.map((t) => `${t.type}:${t.title}`).join(', ') || '(목록 비어 있음)'}`);
  const url = new URL(target.webSocketDebuggerUrl);
  if (!LOCAL_HOSTS.has(url.hostname)) throw new Error(`웹소켓 주소가 로컬이 아니다: ${url.hostname}`);

  const ws = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((resolve, reject) => {
    ws.onopen = resolve;
    ws.onerror = () => reject(new Error('CDP 웹소켓을 열지 못했다'));
  });
  let nextId = 0;
  const pending = new Map();
  ws.onmessage = (event) => {
    const message = JSON.parse(String(event.data));
    const waiter = message.id ? pending.get(message.id) : null;
    if (!waiter) return;
    pending.delete(message.id);
    if (message.error) waiter.reject(new Error(`${waiter.method}: ${message.error.message}`));
    else waiter.resolve(message.result);
  };
  ws.onclose = () => {
    for (const waiter of pending.values()) waiter.reject(new Error('CDP 연결이 닫혔다'));
    pending.clear();
  };

  function send(method, params = {}) {
    return new Promise((resolve, reject) => {
      const id = ++nextId;
      const timer = setTimeout(() => {
        pending.delete(id);
        reject(new Error(`${method}: ${timeoutMs}ms 안에 답이 없다`));
      }, timeoutMs);
      pending.set(id, {
        method,
        resolve: (value) => (clearTimeout(timer), resolve(value)),
        reject: (error) => (clearTimeout(timer), reject(error)),
      });
      ws.send(JSON.stringify({ id, method, params }));
    });
  }

  /** JS 식을 평가해 값을 돌려준다(Promise 는 기다린다). 예외는 던진다 */
  async function evaluate(expression) {
    const result = await send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
    if (result.exceptionDetails) {
      const detail = result.exceptionDetails;
      throw new Error(`JS 예외: ${detail.exception?.description ?? detail.text}`);
    }
    return result.result.value;
  }

  /** PNG 바이트. clip 이 있으면 그 사각형(CSS 픽셀)만 */
  async function screenshot({ clip } = {}) {
    const params = { format: 'png', fromSurface: true, captureBeyondViewport: false };
    if (clip) {
      const viewport = await evaluate('({width: window.innerWidth, height: window.innerHeight})');
      params.clip = { ...normalizeRect(clip, viewport), scale: 1 };
    }
    const { data } = await send('Page.captureScreenshot', params);
    return Buffer.from(data, 'base64');
  }

  /** 선택자 요소의 사각형 — 없으면 null */
  async function rectOf(selector) {
    return evaluate(`(() => { const e = document.querySelector(${JSON.stringify(selector)}); if (!e) return null; const r = e.getBoundingClientRect(); return {x: r.x, y: r.y, width: r.width, height: r.height}; })()`);
  }

  /** 그 사각형만 캡처해 output 에 쓴다. repo 가 있으면 <repo>/e2e/{out,artifacts}/ 아래만 허용한다 */
  async function cropTo(rect, { output, repo } = {}) {
    if (!output) throw new Error('output 이 필요하다');
    const target = repo ? checkedOutput(output, repo) : path.resolve(output);
    const bytes = await screenshot({ clip: rect });
    fs.mkdirSync(path.dirname(target), { recursive: true });
    fs.writeFileSync(target, bytes);
    return { path: target, bytes: bytes.length };
  }

  return { target, send, evaluate, screenshot, rectOf, cropTo, close: () => ws.close() };
}

// ── 명령줄 ─────────────────────────────────────────────────────────────────

export function parseArgs(argv) {
  const flags = {};
  const rest = [];
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === '--port' || arg === '--rect' || arg === '--title' || arg === '--url') {
      if (i + 1 >= argv.length) throw new Error(`${arg} 값이 없다`);
      flags[arg.slice(2)] = argv[(i += 1)];
    } else if (arg.startsWith('--')) {
      throw new Error(`알 수 없는 옵션: ${arg}`);
    } else {
      rest.push(arg);
    }
  }
  return { flags, rest };
}

async function main(argv) {
  let parsed;
  try {
    parsed = parseArgs(argv);
  } catch (error) {
    console.error(`USAGE|${error.message}`);
    return 2;
  }
  const { flags, rest } = parsed;
  const [command, arg] = rest;
  if (!['targets', 'eval', 'shot'].includes(command) || ((command === 'eval' || command === 'shot') && !arg)) {
    console.error('USAGE|node cdp.mjs --port <포트> targets | eval <JS 식> | shot <out.png> [--rect x,y,w,h]');
    return 2;
  }
  const port = flags.port ?? process.env.CDP_PORT;
  try {
    if (command === 'targets') {
      const list = await (await fetch(`http://127.0.0.1:${Number(port)}/json`, { signal: AbortSignal.timeout(15000) })).json();
      console.log(JSON.stringify(list.map((t) => ({ type: t.type, title: t.title, url: t.url })), null, 1));
      return 0;
    }
    const page = await connect({ port, title: flags.title, urlIncludes: flags.url });
    try {
      if (command === 'eval') {
        console.log(JSON.stringify(await page.evaluate(arg), null, 1));
      } else {
        const bytes = await page.screenshot({ clip: flags.rect ? parseRect(flags.rect) : undefined });
        fs.mkdirSync(path.dirname(path.resolve(arg)), { recursive: true });
        fs.writeFileSync(arg, bytes);
        console.log(`SHOT|${path.resolve(arg)}|${bytes.length}`);
      }
    } finally {
      page.close();
    }
    return 0;
  } catch (error) {
    console.error(`ERROR|${error.message}`);
    return 1;
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  // 웹소켓 닫기 인사가 끝나기를 기다리지 않고 끝낸다 — 상대가 닫는 프레임에 답하지 않아도 매달리지 않는다
  process.exit(await main(process.argv.slice(2)));
}
