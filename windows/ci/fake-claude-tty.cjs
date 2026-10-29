'use strict';
// 키를 받아 적는 가짜 Claude Code — 보드 탭 안에서 claude 대신 도는 터미널 앱이다(W3 설계 §8, G7 확인).
// 일시중지(Esc)·재개(문구 + Enter)·중단(Esc, /exit, Enter)·이름 바꾸기(/rename 이름)가 ConPTY 를 지나 앱에 실제로 어떤 바이트로
// 닿는지 키 기록(keylog)으로 확인하는 것이 목적이다. 주고받는 폴더 방식인 fake-claude.cjs 와 달리 표준입력 키로만 움직인다.
//
//   node fake-claude-tty.cjs [claude 인자…]        (G7 에서는 node 사본 claude.exe 로 돈다)
//
// 환경 변수
//   FAKE_CLAUDE_KEYLOG     받은 키 기록 파일(덧붙임). 줄마다 JSON 하나 — 조각(chunk) 기록 {t,ms,gapMs,hex,len,kind} 과
//                          사건 기록 {t,ms,gapMs,event,…}(start·esc·line·rename·exit·hook). 없으면 기록하지 않는다
//   FAKE_CLAUDE_ARGVLOG    시작할 때 argv 한 줄(JSON 배열)을 덧붙인다
//   FAKE_CLAUDE_HOOKS      0 이면 hook 을 부르지 않는다(기본은 settings.json 에 등록된 Claude Board hook 을 부른다)
//   FAKE_CLAUDE_SETTINGS   hook 을 읽을 settings.json 경로(기본 <홈>/.claude/settings.json, 홈은 CLAUDE_BOARD_HOME > USERPROFILE > HOME)
//   FAKE_CLAUDE_HOOK_MARKER hook 을 알아보는 글(command·args 에 들어 있으면 우리 것). 기본 claude-board-hook, .claude/notifier
//   FAKE_CLAUDE_SESSION_ID 세션 id(기본: 인자의 --session-id·--resume 값, 없으면 새로 만든다)
//   FAKE_CLAUDE_MAX_MS     이만큼 지나면 스스로 끝난다(기본 30분 — 확인이 끝내지 못해도 오래 남지 않게)
//
// 환경 파일(D6)
//   이 스크립트 옆 fake-claude.env.json — {"FAKE_CLAUDE_KEYLOG": "C:\\...", …} 로 빠진 변수를 채운다. 실제 환경변수가 이긴다.
//   보드 탭의 셸은 보드의 환경을 물려받지 않아 시험 하니스가 이 파일로 기록 위치를 알려 준다. 채운 변수 이름은 start 사건의 envFilled 에 남는다
//
// 동작
//   시작: 기록, 세션 파일, 대화 기록 파일을 만들고 UserPromptSubmit hook(상태 working)을 부른다
//   조각마다: 16진수와 시각을 기록한다. 조각 하나가 정확히 0x1b 한 바이트면 홀로 온 Esc 로 보고 esc 사건을 남긴다
//   줄(Enter 로 끝남): "/exit" 면 SessionEnd hook 을 부르고 종료 코드 0 으로 끝난다
//                      "/rename 이름" 이면 <홈>/.claude/sessions/<pid>.json 의 name 을 쓰고 "renamed: 이름" 을 되돌려 쓴다
//                      그 밖의 글이면 UserPromptSubmit hook 을 부른다
//   표준입력이 닫히면(파이프 시험) SessionEnd hook 을 부르고 끝난다. Ctrl+C(0x03)는 기록하고 130 으로 끝난다
const fs = require('fs');
const os = require('os');
const path = require('path');
const crypto = require('crypto');
const cp = require('child_process');
const { StringDecoder } = require('string_decoder');

// 환경 파일(D6) — 보드 탭의 셸은 보드의 환경변수를 물려받지 않으므로, 이 스크립트 옆 fake-claude.env.json({"이름": "값"})으로 빠진 변수를 채운다.
// 이미 있는 실제 환경변수는 이기고(덮어쓰지 않는다), 문자열·숫자·불리언 값만 받는다. 없거나 깨진 파일은 무시한다. 다른 것을 읽기 전에 먼저 한다.
const envFilled = [];
try {
  const filled = JSON.parse(fs.readFileSync(path.join(__dirname, 'fake-claude.env.json'), 'utf8').replace(/^﻿/, ''));
  for (const [key, value] of Object.entries(filled && typeof filled === 'object' && !Array.isArray(filled) ? filled : {})) {
    if (process.env[key] === undefined && ['string', 'number', 'boolean'].includes(typeof value)) {
      process.env[key] = String(value);
      envFilled.push(key);
    }
  }
} catch (_) {
  // 환경 파일이 없으면 실제 환경만 쓴다
}

const argv = process.argv.slice(2);
const startedAt = Date.now();
const keylog = process.env.FAKE_CLAUDE_KEYLOG || '';
let lastAt = startedAt;

function append(file, text) {
  try {
    fs.appendFileSync(file, text);
  } catch (_) {
    // 기록 실패가 앱 동작을 막지 않는다
  }
}

function record(entry) {
  const now = Date.now();
  const line = Object.assign({ t: now, ms: now - startedAt, gapMs: now - lastAt }, entry);
  lastAt = now;
  if (keylog) append(keylog, JSON.stringify(line) + '\n');
}

function homeDir() {
  return process.env.CLAUDE_BOARD_HOME || process.env.USERPROFILE || process.env.HOME || os.homedir();
}

function argValue(flags) {
  for (let i = 0; i < argv.length; i += 1) {
    const eq = flags.find((f) => argv[i].startsWith(f + '='));
    if (eq) return argv[i].slice(eq.length + 1);
    if (flags.includes(argv[i]) && argv[i + 1]) return argv[i + 1];
  }
  return '';
}

const sessionId = process.env.FAKE_CLAUDE_SESSION_ID || argValue(['--session-id', '--resume']) || crypto.randomUUID();
const cwd = process.cwd();
const claudeDir = path.join(homeDir(), '.claude');
const transcriptPath = path.join(claudeDir, 'projects', 'fake-claude-tty', `${sessionId}.jsonl`);
const sessionRecordPath = path.join(claudeDir, 'sessions', `${process.pid}.json`);
let name = argValue(['--name', '-n']) || null;

function ensureFile(file, content) {
  try {
    fs.mkdirSync(path.dirname(file), { recursive: true });
    if (!fs.existsSync(file)) fs.writeFileSync(file, content);
  } catch (_) {
    // 홈을 쓸 수 없으면 기록 없이 계속한다
  }
}

// Claude Code 가 도는 세션마다 쓰는 ~/.claude/sessions/<pid>.json — 보드가 카드 이름 라벨과 승인 창 확인(status)에 읽는다
function writeSessionRecord(status) {
  try {
    fs.mkdirSync(path.dirname(sessionRecordPath), { recursive: true });
    const body = { pid: process.pid, sessionId, cwd, startedAt, status };
    if (name) body.name = name;
    fs.writeFileSync(sessionRecordPath, JSON.stringify(body));
  } catch (_) {
    // 무시
  }
}

// ── hook ────────────────────────────────────────────────────────────────────

function ourHooks(eventName) {
  if (process.env.FAKE_CLAUDE_HOOKS === '0') return [];
  const settingsPath = process.env.FAKE_CLAUDE_SETTINGS || path.join(claudeDir, 'settings.json');
  let settings;
  try {
    settings = JSON.parse(fs.readFileSync(settingsPath, 'utf8').replace(/^﻿/, ''));
  } catch (_) {
    return [];
  }
  const markers = process.env.FAKE_CLAUDE_HOOK_MARKER ? [process.env.FAKE_CLAUDE_HOOK_MARKER] : ['claude-board-hook', '.claude/notifier'];
  const found = [];
  for (const group of (settings.hooks && settings.hooks[eventName]) || []) {
    for (const hook of group.hooks || []) {
      const text = `${hook.command || ''} ${(hook.args || []).join(' ')}`.replace(/\\/g, '/');
      if (hook.type === 'command' && hook.command && markers.some((m) => text.includes(m))) found.push(hook);
    }
  }
  return found;
}

// 등록된 hook 을 셸 없이(Claude Code 의 exec 형식처럼) stdin JSON 과 함께 돌린다
function fireHook(eventName, extra) {
  const input = JSON.stringify(Object.assign({ session_id: sessionId, transcript_path: transcriptPath, cwd, hook_event_name: eventName }, extra));
  for (const hook of ourHooks(eventName)) {
    const started = Date.now();
    const r = cp.spawnSync(hook.command, hook.args || [], {
      input,
      cwd,
      timeout: (hook.timeout || 60) * 1000,
      windowsHide: true,
      shell: false,
      encoding: 'utf8',
      env: Object.assign({}, process.env, { CLAUDE_PROJECT_DIR: cwd }),
    });
    record({ event: 'hook', hookEvent: eventName, status: r.status, error: r.error ? String(r.error.code || r.error.message) : null, hookMs: Date.now() - started });
  }
}

// ── 키 ──────────────────────────────────────────────────────────────────────

function kindOf(bytes) {
  if (bytes.length === 1 && bytes[0] === 0x1b) return 'esc';
  if (bytes.length === 1 && bytes[0] === 0x0d) return 'enter';
  if (bytes[0] === 0x1b) return 'seq';
  if (bytes.every((b) => b >= 0x20 && b !== 0x7f)) return 'text';
  if (bytes[bytes.length - 1] === 0x0d && bytes.slice(0, -1).every((b) => b >= 0x20 && b !== 0x7f)) return 'text+enter';
  return 'other';
}

let line = '';
const decoder = new StringDecoder('utf8');

function finish(code) {
  record({ event: 'exit', code });
  process.exit(code);
}

function submit() {
  const text = line.trim();
  line = '';
  record({ event: 'line', text });
  if (text === '/exit') {
    fireHook('SessionEnd', { reason: 'prompt_input_exit' });
    process.stdout.write('\r\nbye\r\n');
    finish(0);
    return;
  }
  const rename = /^\/rename(?:\s+(.*))?$/.exec(text);
  if (rename) {
    const wanted = (rename[1] || '').trim();
    if (wanted) {
      name = wanted;
      writeSessionRecord('idle');
      record({ event: 'rename', name });
      process.stdout.write(`\r\nrenamed: ${name}\r\n`);
    }
    return;
  }
  if (text) {
    writeSessionRecord('busy');
    fireHook('UserPromptSubmit', { prompt: text });
    process.stdout.write(`\r\nreceived: ${text}\r\n`);
  }
}

function onChunk(chunk) {
  const bytes = Buffer.from(chunk);
  record({ hex: bytes.toString('hex'), len: bytes.length, kind: kindOf(bytes) });
  if (bytes.length === 1 && bytes[0] === 0x1b) {
    record({ event: 'esc' });
    return;
  }
  const decoded = decoder.write(bytes);
  for (const ch of decoded) {
    if (ch === '\r' || ch === '\n') {
      submit();
    } else if (ch === '\x1b') {
      break; // 방향키 같은 이스케이프 시퀀스 — 이 조각의 나머지는 글이 아니다
    } else if (ch === '\x03') {
      record({ event: 'ctrl-c' });
      finish(130);
    } else if (ch === '\x7f' || ch === '\b') {
      line = line.slice(0, -1);
    } else if (ch >= ' ') {
      line += ch;
      process.stdout.write(ch);
    }
  }
}

// ── 시작 ────────────────────────────────────────────────────────────────────

if (process.env.FAKE_CLAUDE_ARGVLOG) append(process.env.FAKE_CLAUDE_ARGVLOG, JSON.stringify(argv) + '\n');
ensureFile(transcriptPath, JSON.stringify({ type: 'user', sessionId, message: { role: 'user', content: 'fake' } }) + '\n');
writeSessionRecord('busy');
record({ event: 'start', pid: process.pid, argv, sessionId, tty: Boolean(process.stdin.isTTY), envFilled });

if (process.stdin.isTTY) process.stdin.setRawMode(true);
process.stdin.on('data', onChunk);
process.stdin.on('end', () => {
  fireHook('SessionEnd', { reason: 'other' });
  finish(0);
});
process.stdin.resume();

fireHook('UserPromptSubmit', { prompt: 'fake-claude-tty start' });
process.stdout.write('fake-claude-tty ready\r\n');

setTimeout(() => finish(0), Number(process.env.FAKE_CLAUDE_MAX_MS) || 30 * 60 * 1000).unref();
