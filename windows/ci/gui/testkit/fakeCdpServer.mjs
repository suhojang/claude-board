// 시험용 가짜 CDP 서버 — http(/json)와 손으로 만든 WebSocket 핸드셰이크·프레임만으로 보드의 WebView2 디버그 포트를 흉내 낸다.
// cdp.test.mjs 와 launch-board 시험(stand-in 보드)이 함께 쓴다. 의존 패키지가 없다.
import crypto from 'node:crypto';
import http from 'node:http';

const GUID = '258EAFA5-E914-47DA-95CA-C5AB0DC85B11';

export function frame(text) {
  const payload = Buffer.from(text, 'utf8');
  let head;
  if (payload.length < 126) head = Buffer.from([0x81, payload.length]);
  else if (payload.length < 65536) head = Buffer.from([0x81, 126, payload.length >> 8, payload.length & 255]);
  else {
    head = Buffer.alloc(10);
    head[0] = 0x81;
    head[1] = 127;
    head.writeBigUInt64BE(BigInt(payload.length), 2);
  }
  return Buffer.concat([head, payload]);
}

// 클라이언트가 보낸(마스크된) 텍스트 프레임들을 꺼낸다 — 짧은 메시지와 126 길이만 다룬다
export function readFrames(state, chunk) {
  state.buffer = Buffer.concat([state.buffer, chunk]);
  const messages = [];
  for (;;) {
    const b = state.buffer;
    if (b.length < 2) break;
    let length = b[1] & 0x7f;
    let offset = 2;
    if (length === 126) {
      if (b.length < 4) break;
      length = b.readUInt16BE(2);
      offset = 4;
    }
    if (b.length < offset + 4 + length) break;
    const mask = b.subarray(offset, offset + 4);
    const data = Buffer.from(b.subarray(offset + 4, offset + 4 + length));
    for (let i = 0; i < data.length; i += 1) data[i] ^= mask[i % 4];
    if ((b[0] & 0x0f) === 1) messages.push(data.toString('utf8'));
    state.buffer = b.subarray(offset + 4 + length);
  }
  return messages;
}

export const boardTargets = (port) => [{ type: 'page', title: 'Claude Board', url: 'http://tauri.localhost/', webSocketDebuggerUrl: `ws://127.0.0.1:${port}/devtools/page/1` }];

/**
 * targets(port) → /json 의 목록, handler(message) → 명령에 대한 답({result}|{error}, 답하지 않으려면 undefined).
 * port 0 이면 빈 포트를 고른다. 127.0.0.1 에만 연다.
 */
export async function serveFakeBoard({ targets = boardTargets, handler, port = 0 }) {
  const calls = [];
  const server = http.createServer((req, res) => {
    if (req.url === '/json') {
      res.setHeader('content-type', 'application/json');
      res.end(JSON.stringify(targets(server.address().port)));
    } else {
      res.statusCode = 404;
      res.end();
    }
  });
  const sockets = new Set(); // 업그레이드된 연결은 http 서버가 추적하지 않아, 닫을 때 직접 끊는다
  server.on('upgrade', (req, socket) => {
    sockets.add(socket);
    socket.on('close', () => sockets.delete(socket));
    const accept = crypto.createHash('sha1').update(req.headers['sec-websocket-key'] + GUID).digest('base64');
    socket.write(`HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: ${accept}\r\n\r\n`);
    const state = { buffer: Buffer.alloc(0) };
    socket.on('data', (chunk) => {
      for (const text of readFrames(state, chunk)) {
        const message = JSON.parse(text);
        calls.push(message);
        const reply = handler(message);
        if (reply !== undefined) socket.write(frame(JSON.stringify({ id: message.id, ...reply })));
      }
    });
    socket.on('error', () => {});
  });
  await new Promise((resolve) => server.listen(port, '127.0.0.1', resolve));
  return {
    port: server.address().port,
    calls,
    close: () =>
      new Promise((resolve) => {
        for (const socket of sockets) socket.destroy();
        server.closeAllConnections?.();
        server.close(resolve);
      }),
  };
}
