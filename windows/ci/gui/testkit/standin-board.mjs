// launch-board.ps1 시험용 "보드 대역" — 진짜 보드를 띄우지 않고 launcher 의 확인 절차(우리 것인지, 포트, test_claude_prefix)를 돌려 본다.
// node 사본을 ClaudeBoard.exe 라는 이름으로 두고 이 스크립트를 인자로 준다. WebView2 처럼 WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS 의
// --remote-debugging-port 에서 /json 과 CDP(Runtime.evaluate)로 답한다. 화면은 없다.
//
// STANDIN_MODE   ok(기본) | swallow(단일 인스턴스에 삼켜진 것처럼 바로 종료 0) | wrongprefix(test_claude_prefix 가 늘 null) | silent(포트를 열지 않고 산다)
// STANDIN_ENV_OUT  있으면 받은 환경변수 몇 개를 이 파일에 JSON 으로 쓴다
import fs from 'node:fs';
import { serveFakeBoard } from './fakeCdpServer.mjs';

const mode = process.env.STANDIN_MODE || 'ok';
const port = Number(/--remote-debugging-port=(\d+)/.exec(process.env.WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS || '')?.[1] ?? 0);

if (process.env.STANDIN_ENV_OUT) {
  const keys = ['CLAUDE_BOARD_HOME', 'WEBVIEW2_USER_DATA_FOLDER', 'WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS', 'CLAUDE_BOARD_CLAUDE', 'CLAUDE_BOARD_NOTIFIER_DIR', 'CLAUDE_CONFIG_DIR'];
  fs.writeFileSync(process.env.STANDIN_ENV_OUT, JSON.stringify(Object.fromEntries(keys.map((k) => [k, process.env[k] ?? null]))));
}

if (mode === 'swallow') process.exit(0);

if (mode !== 'silent' && port > 0) {
  const prefix = () => {
    if (mode === 'wrongprefix') return null;
    const claude = process.env.CLAUDE_BOARD_CLAUDE;
    return claude && process.env.CLAUDE_BOARD_HOME ? claude.split('|') : null;
  };
  try {
    await serveFakeBoard({
      port,
      handler: (message) => {
        const expression = String(message.params?.expression ?? '');
        if (message.method === 'Runtime.evaluate' && expression.includes('test_claude_prefix')) return { result: { result: { type: 'object', value: prefix() } } };
        return { error: { message: 'stand-in: unsupported ' + message.method } };
      },
    });
  } catch (error) {
    // 포트가 이미 남의 것이면 열지 못한다 — launcher 가 "포트를 쥔 것이 우리 자손이 아님" 을 잡는지 보려는 경우
    fs.writeSync(2, `stand-in: ${error.message}\n`);
  }
}

setInterval(() => {}, 1 << 30); // 끌 때까지 산다
