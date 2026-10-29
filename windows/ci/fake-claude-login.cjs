'use strict';
// `claude auth login --claudeai` 를 흉내 내는 가짜 — 보드의 계정 추가 로그인(D8)을 진짜 로그인·진짜 계정 없이 시험한다.
// 진짜 claude 가 아니고 네트워크를 쓰지 않는다. 로그인 주소는 예약된 `.test` 최상위 도메인이라 어디에도 닿지 않는다.
//
//   node fake-claude-login.cjs auth login --claudeai [--email 주소]
//
// 출력(맥 보드의 loginOutput·loginEnded 가 읽는 모양 그대로)
//   로그인 주소 줄    `https://login.fake.test/oauth/authorize?...` — 뒤에 줄바꿈이 있다(맥은 주소 뒤 공백까지를 주소로 읽는다)
//   코드 입력 안내    `Paste code here if prompted > ` 에서 코드를 기다린다
//   틀린 코드         같은 줄에 이어서 `Login failed: ...` 를 쓰고 종료 코드 1 (맥은 `prompted >` 뒤 마지막 줄을 실패 까닭으로 보인다)
//   맞는 코드         CLAUDE_CONFIG_DIR 폴더에 계정·토큰 파일을 쓰고 `Login successful.` 후 종료 코드 0
//
// 환경 변수
//   CLAUDE_CONFIG_DIR     (필수) 로그인 자리. 없으면 아무것도 쓰지 않고 종료 코드 2 — 사용자 ~/.claude 를 절대 건드리지 않는다
//   FAKE_LOGIN_CODE       맞는 코드(기본 fake-code-1234)
//   FAKE_LOGIN_EMAIL      계정 이메일(기본 fake-login@example.test, --email 인자가 있으면 그 값)
//   FAKE_LOGIN_AUTO_MS    0 보다 크면 코드 없이 그만큼 뒤에 스스로 성공한다(localhost 콜백으로 저절로 끝나는 길)
//   FAKE_LOGIN_LOG        기록 파일(덧붙임, 줄마다 JSON 하나 — start·url·code·write·exit). 없으면 기록하지 않는다
//   FAKE_LOGIN_MAX_MS     이만큼 지나면 스스로 실패로 끝난다(기본 15분)
//
// 종료 코드  0 성공 · 1 로그인 실패(틀린 코드·입력 닫힘·시간 초과) · 2 사용법·자리 없음 · 130 Ctrl+C
const fs = require('fs');
const path = require('path');
const crypto = require('crypto');

const argv = process.argv.slice(2);
const logFile = process.env.FAKE_LOGIN_LOG || '';

function record(entry) {
  if (!logFile) return;
  try {
    fs.appendFileSync(logFile, JSON.stringify(Object.assign({ t: Date.now() }, entry)) + '\n');
  } catch (_) {
    // 기록 실패가 흉내를 막지 않는다
  }
}

function out(text) {
  process.stdout.write(text);
}

function finish(code) {
  record({ event: 'exit', code });
  process.exitCode = code;
  // 표준 출력이 비워진 뒤 끝낸다(파이프·ConPTY 에서 마지막 줄이 잘리지 않게)
  setTimeout(() => process.exit(code), 30);
}

function argValue(flag) {
  const at = argv.indexOf(flag);
  return at >= 0 && argv[at + 1] ? argv[at + 1] : '';
}

const slot = process.env.CLAUDE_CONFIG_DIR || '';
const rightCode = process.env.FAKE_LOGIN_CODE || 'fake-code-1234';
const email = argValue('--email') || process.env.FAKE_LOGIN_EMAIL || 'fake-login@example.test';
const autoMs = Number(process.env.FAKE_LOGIN_AUTO_MS) || 0;

record({ event: 'start', argv, slot, pid: process.pid });

if (argv[0] !== 'auth' || argv[1] !== 'login') {
  out('usage: fake-claude-login auth login --claudeai [--email <address>]\r\n');
  finish(2);
} else if (!slot || !path.isAbsolute(slot)) {
  out('fake-claude-login: CLAUDE_CONFIG_DIR 가 없거나 절대 경로가 아니다 — 아무것도 쓰지 않는다\r\n');
  finish(2);
} else {
  run();
}

// 맞는 코드를 받았을 때만 로그인 자리에 쓴다 — 보드가 읽는 두 파일(.credentials.json 의 claudeAiOauth, .claude.json 의 oauthAccount)
function writeSlot() {
  fs.mkdirSync(slot, { recursive: true });
  const uuid = crypto.createHash('sha1').update(email).digest('hex').replace(/^(.{8})(.{4})(.{4})(.{4})(.{12}).*$/, '$1-$2-$3-$4-$5');
  const credentials = {
    claudeAiOauth: {
      accessToken: 'fake-access-' + crypto.randomBytes(6).toString('hex'),
      refreshToken: 'fake-refresh-' + crypto.randomBytes(6).toString('hex'),
      expiresAt: Date.now() + 8 * 3600 * 1000,
      refreshTokenExpiresAt: Date.now() + 90 * 24 * 3600 * 1000,
    },
  };
  const account = {
    oauthAccount: {
      accountUuid: uuid,
      organizationUuid: 'fake-org',
      emailAddress: email,
      organizationName: 'Fake Org',
      organizationType: 'claude_max',
      organizationRateLimitTier: 'default_claude_max_5x',
    },
  };
  fs.writeFileSync(path.join(slot, '.credentials.json'), JSON.stringify(credentials));
  fs.writeFileSync(path.join(slot, '.claude.json'), JSON.stringify(account));
  record({ event: 'write', email, accountUuid: uuid });
}

function succeed() {
  try {
    writeSlot();
  } catch (e) {
    out(`\r\nLogin failed: ${e.message}\r\n`);
    finish(1);
    return;
  }
  out('\r\nLogin successful.\r\n');
  finish(0);
}

function run() {
  const state = crypto.randomBytes(8).toString('hex');
  const url = `https://login.fake.test/oauth/authorize?client_id=fake&response_type=code&scope=user%3Aprofile&state=${state}`;
  record({ event: 'url', url });
  out('Opening browser to sign in...\r\n');
  out(`If the browser did not open, visit: ${url}\r\n\r\n`);
  out('Paste code here if prompted > ');

  let typed = '';
  let done = false;
  const stop = (code) => {
    if (done) return;
    done = true;
    finish(code);
  };

  function submit(code) {
    record({ event: 'code', code });
    if (code === rightCode) {
      done = true;
      succeed();
    } else {
      out('Login failed: Invalid authorization code\r\n');
      stop(1);
    }
  }

  if (process.stdin.isTTY) process.stdin.setRawMode(true);
  process.stdin.setEncoding('utf8');
  process.stdin.on('data', (chunk) => {
    if (done) return;
    for (const ch of chunk) {
      if (done) return;
      if (ch === '\x03') {
        record({ event: 'ctrl-c' });
        stop(130);
      } else if (ch === '\r' || ch === '\n') {
        const code = typed.trim();
        typed = '';
        if (code) submit(code); // 빈 줄은 코드가 아니다 — 계속 기다린다
      } else if (ch === '\x7f' || ch === '\b') {
        typed = typed.slice(0, -1);
      } else if (ch >= ' ') {
        typed += ch;
      }
    }
  });
  process.stdin.on('end', () => {
    if (done) return;
    out('\r\nLogin cancelled.\r\n');
    stop(1);
  });
  process.stdin.resume();

  if (autoMs > 0) {
    setTimeout(() => {
      if (done) return;
      done = true;
      record({ event: 'auto' });
      succeed();
    }, autoMs);
  }
  setTimeout(() => {
    if (done) return;
    out('\r\nLogin failed: timed out\r\n');
    stop(1);
  }, Number(process.env.FAKE_LOGIN_MAX_MS) || 15 * 60 * 1000).unref();
}
