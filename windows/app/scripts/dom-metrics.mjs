// DOM 렌더러(WebGL 이 없을 때)의 칸 정렬 실측. 실제 src/app.css 와 TerminalPane.svelte 의 .term-host 규칙으로 페이지를 만들어
// 헤드리스 Chrome 에서 xterm 을 띄우고, 80칸 줄의 마지막 칸이 칸 격자에서 벗어난 정도와 입력 textarea-커서 간격을 잰다.
// 사용: node scripts/dom-metrics.mjs [--svelte=<다른 TerminalPane.svelte>] [--dprs=1,1.25,1.5] [--tol=0.5]
// 환경: CHROME(chrome.exe 경로, 기본 C:\Program Files\Google\Chrome\Application\chrome.exe),
//       DOM_METRICS_REQUIRED(설정하면 Chrome 이 없을 때 건너뛰지 않고 실패)
// 종료코드: 0 통과(또는 Chrome 없어 건너뜀) · 1 어긋남 · 2 실행 오류
import { spawn, execSync } from 'node:child_process';
import { mkdtempSync, writeFileSync, readFileSync, rmSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, dirname, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import net from 'node:net';

const app = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const args = process.argv.slice(2);
const opt = k => (args.find(a => a.startsWith('--' + k + '=')) || '').split('=')[1];
const dprs = (opt('dprs') || '1,1.25,1.5').split(',').map(Number);
const TOL = Number(opt('tol') || 0.5);
const CHROME = process.env.CHROME || 'C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe';
const sveltePath = opt('svelte') ? resolve(opt('svelte')) : join(app, 'src/lib/terminal/TerminalPane.svelte');

if (!existsSync(CHROME)) {
  const msg = `dom-metrics: Chrome 없음 (${CHROME})`;
  if (process.env.DOM_METRICS_REQUIRED) { console.error(msg + ' — DOM_METRICS_REQUIRED 라 실패'); process.exit(1); }
  console.log(msg + ' — 건너뜀');
  process.exit(0);
}

// .pane :global(.term-host …) { … } 규칙을 일반 CSS 로 꺼낸다(.term-host 가 든 규칙만).
const svelteCss = readFileSync(sveltePath, 'utf8').match(/<style[^>]*>([\s\S]*?)<\/style>/)[1];
const termRules = [...svelteCss.matchAll(/\.pane\s+:global\(([^)]*\.term-host[^)]*)\)\s*\{[^}]*\}/g)]
  .map(m => m[0].replace(/\.pane\s+:global\(([^)]*)\)/, '.pane $1')).join('\n');
if (!termRules) { console.error('dom-metrics: .term-host 규칙을 못 찾음'); process.exit(2); }
const appCss = readFileSync(join(app, 'src/app.css'), 'utf8');
const mod = p => pathToFileURL(join(app, 'node_modules', p)).href;

const PAGE_JS = `
const THEME={background:"#212121",foreground:"#ebebf5",cursor:"#d97857",cursorAccent:"#212121"};
const term=new Terminal({allowProposedApi:true,scrollback:5000,cursorBlink:true,fontSize:14,
  fontFamily:"Cascadia Mono, Consolas, D2Coding, monospace",theme:THEME,cols:80,rows:10,windowsPty:{backend:"conpty"}});
term.loadAddon(new Unicode11Addon.Unicode11Addon()); term.unicode.activeVersion='11';
term.open(document.getElementById('host')); term.focus();
const raf=()=>new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(()=>setTimeout(r,60))));
const write=s=>new Promise(r=>term.write(s,r));
const wcw=ch=>{const c=ch.codePointAt(0);return (c>=0x1100&&c<=0x115f)||(c>=0x2e80&&c<=0xa4cf)||(c>=0xac00&&c<=0xd7a3)||(c>=0xff00&&c<=0xff60)?2:1;};
function glyphs(line){
  const row=document.querySelector('.xterm-rows').children[0];
  const w=document.createTreeWalker(row,NodeFilter.SHOW_TEXT);
  const out=[];let col=0,i=0,n;const chars=[...line];
  while((n=w.nextNode())){for(let k=0;k<n.data.length;k++){
    const ch=chars[i];if(ch===undefined)break;
    const r=document.createRange();r.setStart(n,k);r.setEnd(n,k+1);
    out.push({ch,col,left:r.getBoundingClientRect().left});col+=wcw(ch);i++;}}
  return out;
}
const D='0123456789'.repeat(8);
const LINES={ascii:D,mixed:'가나다abc한글Terminal테스트라마바사아자ABCDEFG가나다라마바사abcdefghijklmnopqrst'};
window.measure=async kind=>{
  term.reset();
  let c=0,text='';for(const ch of LINES[kind]){if(c+wcw(ch)>80)break;text+=ch;c+=wcw(ch);}
  await write(text);await raf();
  const cw=term._core._renderService.dimensions.css.cell.width;
  const rowsLeft=document.querySelector('.xterm-rows').getBoundingClientRect().left;
  const g=glyphs(text);const last=g[g.length-1];
  const cur=document.querySelector('.xterm-rows .xterm-cursor');
  const ta=term.textarea.getBoundingClientRect();
  return {kind,dpr:devicePixelRatio,cellW:cw,lastCol:last.col,
    drift:(last.left-rowsLeft)-last.col*cw,
    taMinusCursor:cur?ta.left-cur.getBoundingClientRect().left:null,
    rowsLS:document.querySelector('.xterm-rows').children[0].style.letterSpacing};
};
window.ready=true;`;

const html = `<!doctype html><html><head><meta charset="utf-8">
<link rel="stylesheet" href="${mod('@xterm/xterm/css/xterm.css')}">
<style>${appCss}</style>
<style>body{margin:0;background:#212121;height:100vh;overflow:hidden}.pane{position:relative;width:100%;height:100%}</style>
<style>${termRules}</style>
<script src="${mod('@xterm/xterm/lib/xterm.js')}"></script>
<script src="${mod('@xterm/addon-unicode11/lib/addon-unicode11.js')}"></script>
</head><body><div class="pane"><div class="term-host" id="host"></div></div><script>${PAGE_JS}</script></body></html>`;

const freePort = () => new Promise(r => { const s = net.createServer().listen(0, () => { const p = s.address().port; s.close(() => r(p)); }); });
const port = await freePort();
const work = mkdtempSync(join(tmpdir(), 'dom-metrics-'));
const pageFile = join(work, 'page.html');
writeFileSync(pageFile, html);
const chrome = spawn(CHROME, ['--headless=new', `--remote-debugging-port=${port}`, `--user-data-dir=${join(work, 'profile')}`,
  '--disable-gpu', '--disable-webgl', '--no-first-run', '--allow-file-access-from-files', 'about:blank'], { stdio: 'ignore' });
const cleanup = () => {
  try { execSync(`taskkill /PID ${chrome.pid} /T /F`, { stdio: 'ignore' }); } catch {}
  try { rmSync(work, { recursive: true, force: true, maxRetries: 5 }); } catch {}
};
process.on('exit', cleanup);
let exitCode = 2;
try {
  let list;
  for (let i = 0; i < 100; i++) {
    try { list = await (await fetch(`http://127.0.0.1:${port}/json`)).json(); if (list.some(p => p.type === 'page')) break; } catch {}
    await new Promise(r => setTimeout(r, 100));
  }
  const ws = new WebSocket(list.find(p => p.type === 'page').webSocketDebuggerUrl);
  await new Promise(r => ws.onopen = r);
  let id = 0; const pend = new Map();
  ws.onmessage = e => { const m = JSON.parse(e.data); if (m.id && pend.has(m.id)) { pend.get(m.id)(m); pend.delete(m.id); } };
  const send = (method, params = {}) => new Promise((res, rej) => {
    const i = ++id;
    pend.set(i, m => m.error ? rej(new Error(method + ': ' + m.error.message)) : res(m.result));
    ws.send(JSON.stringify({ id: i, method, params }));
  });
  const ev = async expr => {
    const r = await send('Runtime.evaluate', { expression: expr, awaitPromise: true, returnByValue: true });
    if (r.exceptionDetails) throw new Error(JSON.stringify(r.exceptionDetails));
    return r.result.value;
  };
  await send('Page.enable'); await send('Emulation.setFocusEmulationEnabled', { enabled: true }); await send('Runtime.enable');

  const rows = [];
  for (const dpr of dprs) {
    await send('Emulation.setDeviceMetricsOverride', { width: 1000, height: 500, deviceScaleFactor: dpr, mobile: false });
    await send('Page.navigate', { url: pathToFileURL(pageFile).href });
    for (let i = 0; i < 100 && !(await ev('window.ready === true').catch(() => false)); i++) await new Promise(r => setTimeout(r, 100));
    for (const kind of ['ascii', 'mixed']) {
      const r = await ev(`window.measure(${JSON.stringify(kind)})`);
      r.pass = Math.abs(r.drift) < TOL && r.taMinusCursor != null && Math.abs(r.taMinusCursor) < TOL;
      rows.push(r);
    }
  }
  const f = n => (n == null ? 'n/a' : n.toFixed(2)).padStart(6);
  console.log('dpr   kind   cellW  col  drift  ta-cur  rowLS  result');
  for (const r of rows) console.log(`${String(r.dpr).padEnd(5)} ${r.kind.padEnd(6)} ${f(r.cellW)} ${String(r.lastCol).padStart(3)} ${f(r.drift)} ${f(r.taMinusCursor)}  ${(r.rowsLS || '-').padEnd(6)} ${r.pass ? 'PASS' : 'FAIL'}`);
  const fails = rows.filter(r => !r.pass);
  console.log(`${fails.length ? 'FAIL' : 'PASS'}: ${rows.length - fails.length}/${rows.length} (허용 ${TOL}px)`);
  exitCode = fails.length ? 1 : 0;
  ws.close();
} catch (e) { console.error(e); exitCode = 2; }
cleanup();
process.exit(exitCode);
