'use strict';
// `claude -p --input-format stream-json --output-format stream-json` 를 흉내 내는 가짜 — 자율주행 엔진 시험용(모델 비용 0).
// 진짜 claude 가 아니고 네트워크를 쓰지 않는다. 쓰는 곳은 작업 폴더(cwd) 안의 스크립트가 시킨 파일뿐이다.
// 맥 판 `docs/자율주행/시험 도구/가짜 claude.py` 의 stream 모양을 따른다. 켜는 쪽(엔진 시험)이 `CLAUDE_BOARD_CLAUDE` 와
// `CLAUDE_BOARD_HOME` 을 함께 줄 때만 보드가 이 파일을 claude 자리에 둔다(windows/docs/SPEC.md §5.13). 이 파일은 그 판정을 하지 않는다.
//
//   node fake-claude-stream.cjs [--session-id <uuid> | --resume <uuid>] [--settings <파일>] [--append-system-prompt-file <파일>]
//                               [--model <이름>] [--add-dir <폴더>] [--plugin-dir <폴더>] [그 밖의 인자는 무시]
//
// 입력(표준 입력, 줄마다 JSON 하나)
//   {"type":"user","message":{"role":"user","content":"글" | [{"type":"text","text":"글"}]}}   사용자 차례. 줄 하나가 한 턴이다
//   {"type":"control_request","request_id":"…","request":{"subtype":"interrupt"}}              멈춤 — 아래 「멈춤」
//   입력이 닫히면 하던 턴을 마치고 종료 코드 0 으로 끝난다
//
// 출력(표준 출력, 줄마다 JSON 하나) — 첫 사용자 줄을 받은 뒤 system init 을 낸다(진짜 claude 와 같다)
//   system init → rate_limit_event → assistant(글) → [assistant tool_use + user tool_result 쓰기마다] → result
//   result 는 subtype success, total_cost_usd, num_turns, session_id 를 가진다
//
// 환경 변수
//   FAKE_CLAUDE_LOG      기록 파일(덧붙임, 줄마다 JSON 하나 — start·in·turn·out-of-cwd·signal·exit). start 의 argv 가 받은 인자 전부다
//   FAKE_CLAUDE_SCRIPT   턴별 동작 JSON 파일(배열, 또는 {"turns":[…]}). 없으면 모든 턴이 평범한 한 턴이다
//   FAKE_CLAUDE_STATE    턴 번호를 프로세스 사이에 잇는 파일(기본 <SCRIPT>.state.json). 엔진은 턴마다 프로세스를 새로 띄우므로 필요하다
//   FAKE_CLAUDE_MAX_MS   이만큼 지나면 스스로 끝난다(기본 10분 — 시험이 끝내지 못해도 오래 남지 않게). 종료 코드 1
//
// 턴 동작(스크립트 배열의 항목, 모두 선택). 하는 차례: 사용률 사건 → 글 → delay_ms → 쓰기 → 끝맺음. 스크립트가 모자라면 평범한 턴이다
//   text         assistant 글
//   usage        {five_hour, seven_day} 사용률 0~1 (기본 0.03, 0.10) — rate_limit_event 에 실린다
//   delay_ms     이만큼 기다린다(멈춤을 받으면 바로 깬다)
//   writes       [{path, content}] 작업 폴더 안 파일 쓰기. tool_use(Write) → 실제 쓰기 → tool_result. 폴더 밖이면 쓰지 않고 tool_result is_error
//   limit        한도 거절 모양 "A" | "B" | "C" | "ABC" — 설계.md 4장 「한도 거절 가리기」
//                  A rate_limit_event 의 status rejected 만(result 는 평범한 성공)
//                  B assistant 사건의 error rate_limit·api_error_status 429 만(result 는 평범한 성공)
//                  C result 의 is_error true·api_error_status 429·api_error usage_limit_reached 만(subtype 은 success)
//                  ABC 셋 다(2026-10-14 실제 stream 기록의 모양)
//   resets_in_s  거절의 초기화까지 남은 초(기본 61)
//   empty        true 면 글도 일도 없이 빈 result(num_turns 0, result "") 만 낸다 — 이어 연 직후 나오는 빈 result
//   cost         result 의 total_cost_usd(기본 0.01)
//   num_turns    result 의 num_turns(기본 1 + 쓰기 수)
//   exit_code    result 를 낸 뒤 입력이 닫히기를 기다리지 않고 이 종료 코드로 끝낸다
//   crash        true 면 init 만 내고 result 없이 exit_code(기본 1)로 끝낸다
//
// 멈춤: control_request interrupt 를 받으면 control_response 를 낸 뒤 하던 턴을 거기서 끊고(쉬는 중이어도 바로 깬다)
//   result subtype error_during_execution·is_error true·terminal_reason aborted_tools 를 내고 종료 코드 130 으로 끝낸다.
//   턴이 없을 때(입력을 기다리는 중) 받아도 같다
//
// 종료 코드  0 입력이 닫혀 정상 · 1 시간 초과 · 130 멈춤 · 143 SIGTERM · exit_code/crash 가 정한 값
const fs = require('fs');
const path = require('path');
const crypto = require('crypto');

const argv = process.argv.slice(2);

// 진짜 claude 처럼 버전만 묻고 끝난다 — `claude-autopilot start` 의 준비 확인이 이 한 줄로 claude 가 있는지 본다
if (argv.includes('--version')) {
  process.stdout.write('9.9.9 (Claude Code)\n');
  process.exit(0);
}

const startedAt = Date.now();
const logFile = process.env.FAKE_CLAUDE_LOG || '';
const cwd = process.cwd();

function record(entry) {
  if (!logFile) return;
  try {
    fs.appendFileSync(logFile, JSON.stringify(Object.assign({ t: Date.now(), pid: process.pid }, entry)) + '\n');
  } catch (_) {
    // 기록 실패가 흉내를 막지 않는다
  }
}

function argValue(flag) {
  const at = argv.indexOf(flag);
  return at >= 0 && at + 1 < argv.length ? argv[at + 1] : '';
}

function readText(file) {
  if (!file) return null;
  try {
    // 설정 파일은 거부 규칙이 쓰기 범위마다 붙어 8000자를 넘는다 — 잘리면 e2e 가 JSON 으로 읽지 못한다
    return fs.readFileSync(file, 'utf8').slice(0, 200000);
  } catch (e) {
    return `읽지 못함: ${e.message}`;
  }
}

const resume = argv.includes('--resume');
const sessionId = argValue('--resume') || argValue('--session-id') || crypto.randomUUID();

record({
  event: 'start',
  argv,
  cwd,
  sessionId,
  resume,
  model: argValue('--model') || null,
  settings: readText(argValue('--settings')),
  systemPrompt: readText(argValue('--append-system-prompt-file')),
});

// ---- 출력 ----
function out(object) {
  process.stdout.write(JSON.stringify(object) + '\n');
}

function exitNow(code) {
  record({ event: 'exit', code });
  // 표준 출력이 비워진 뒤 끝낸다(파이프에서 마지막 줄이 잘리지 않게)
  process.stdout.write('', () => process.exit(code));
}

// ---- 스크립트·턴 번호 ----
function loadTurns() {
  const file = process.env.FAKE_CLAUDE_SCRIPT || '';
  if (!file) return [];
  try {
    const parsed = JSON.parse(fs.readFileSync(file, 'utf8').replace(/^﻿/, ''));
    return Array.isArray(parsed) ? parsed : Array.isArray(parsed.turns) ? parsed.turns : [];
  } catch (e) {
    record({ event: 'script-error', message: e.message });
    return [];
  }
}

const scriptTurns = loadTurns();
const stateFile = process.env.FAKE_CLAUDE_STATE || (process.env.FAKE_CLAUDE_SCRIPT ? process.env.FAKE_CLAUDE_SCRIPT + '.state.json' : '');
let localTurns = 0;

// 턴이 끊기거나 죽어도 번호는 쓴 것으로 센다 — 이어 열면 스크립트의 다음 턴부터 한다
function nextTurnIndex() {
  if (!stateFile) return localTurns++;
  let used = 0;
  try {
    used = Number(JSON.parse(fs.readFileSync(stateFile, 'utf8')).turns) || 0;
  } catch (_) {
    // 처음이다
  }
  fs.writeFileSync(stateFile, JSON.stringify({ turns: used + 1 }));
  return used;
}

// ---- 멈춤 ----
let interrupted = false;
let busy = false;
const wakers = new Set();

function sleep(ms) {
  return new Promise((resolve) => {
    const timer = setTimeout(done, ms);
    function done() {
      clearTimeout(timer);
      wakers.delete(done);
      resolve();
    }
    wakers.add(done);
  });
}

function finishInterrupted() {
  out({
    type: 'result',
    subtype: 'error_during_execution',
    is_error: true,
    num_turns: 1,
    terminal_reason: 'aborted_tools',
    stop_reason: 'tool_use',
    result: '',
    total_cost_usd: 0.002,
    session_id: sessionId,
  });
  exitNow(130);
}

function onInterrupt(requestId) {
  record({ event: 'interrupt', busy });
  out({ type: 'control_response', response: { subtype: 'success', request_id: requestId } });
  interrupted = true;
  for (const wake of [...wakers]) wake();
  if (!busy) finishInterrupted();
}

// ---- 한 턴 ----
let initDone = false;

function windowsOf(usage, resetsInS) {
  const now = Math.floor(Date.now() / 1000);
  return {
    five_hour: { utilization: usage.five_hour, resetsAt: now + 3000 },
    seven_day: { utilization: usage.seven_day, resetsAt: now + resetsInS },
  };
}

function rateLimitEvent(status, usage, resetsInS, rateLimitType) {
  const windows = windowsOf(usage, resetsInS);
  const info = windows[rateLimitType];
  return {
    type: 'rate_limit_event',
    rate_limit_info: {
      status,
      resetsAt: info.resetsAt,
      rateLimitType,
      utilization: info.utilization,
      overageStatus: status === 'rejected' ? 'rejected' : 'allowed',
      isUsingOverage: false,
      unifiedWindows: windows,
    },
    session_id: sessionId,
  };
}

function assistantText(text, extra) {
  return Object.assign(
    { type: 'assistant', message: { role: 'assistant', model: 'fake', content: [{ type: 'text', text }] }, parent_tool_use_id: null, session_id: sessionId },
    extra,
  );
}

// 작업 폴더 안일 때만 그 절대 경로를 돌려준다
function insideCwd(target) {
  const full = path.resolve(cwd, target);
  const relative = path.relative(cwd, full);
  return relative && !relative.startsWith('..') && !path.isAbsolute(relative) ? full : null;
}

function writeFile(spec, index) {
  const toolId = `toolu_fake_${index}`;
  out({
    type: 'assistant',
    message: { role: 'assistant', model: 'fake', content: [{ type: 'tool_use', id: toolId, name: 'Write', input: { file_path: spec.path, content: spec.content } }] },
    parent_tool_use_id: null,
    session_id: sessionId,
  });
  const full = insideCwd(String(spec.path || ''));
  let content = `File created successfully at: ${spec.path}`;
  let isError = false;
  if (!full) {
    isError = true;
    content = `작업 폴더 밖이라 쓰지 않았다: ${spec.path}`;
    record({ event: 'out-of-cwd', path: spec.path });
  } else {
    try {
      fs.mkdirSync(path.dirname(full), { recursive: true });
      fs.writeFileSync(full, String(spec.content === undefined ? '' : spec.content));
    } catch (e) {
      isError = true;
      content = `쓰지 못했다: ${e.message}`;
    }
  }
  out({
    type: 'user',
    message: { role: 'user', content: [{ type: 'tool_result', tool_use_id: toolId, content, is_error: isError }] },
    parent_tool_use_id: null,
    session_id: sessionId,
  });
}

function emptyResult() {
  out({ type: 'result', subtype: 'success', is_error: false, duration_api_ms: 0, num_turns: 0, stop_reason: null, session_id: sessionId, total_cost_usd: 0, result: '' });
}

async function runTurn(userText) {
  const index = nextTurnIndex();
  const turn = scriptTurns[index] || {};
  record({ event: 'turn', index, userText: String(userText).slice(0, 200), turn });

  if (!initDone) {
    initDone = true;
    out({ type: 'system', subtype: 'init', session_id: sessionId, cwd, model: argValue('--model') || 'fake', tools: [], mcp_servers: [] });
  }
  if (turn.crash) {
    exitNow(turn.exit_code === undefined ? 1 : turn.exit_code);
    return;
  }
  if (turn.empty) {
    emptyResult();
    return afterTurn(turn);
  }

  const limit = String(turn.limit || '');
  const resetsInS = turn.resets_in_s === undefined ? 61 : turn.resets_in_s;
  const usage = Object.assign({ five_hour: 0.03, seven_day: 0.1 }, turn.usage);
  const hitUsage = limit ? Object.assign({}, usage, { seven_day: 1 }) : usage;
  const wantsA = limit === 'A' || limit === 'ABC';
  const wantsB = limit === 'B' || limit === 'ABC';
  const wantsC = limit === 'C' || limit === 'ABC';

  if (wantsA) out(rateLimitEvent('rejected', hitUsage, resetsInS, 'seven_day'));
  else out(rateLimitEvent('allowed', hitUsage, resetsInS, 'five_hour'));

  const limitText = "You've hit your weekly limit · resets 4pm (Asia/Seoul)";
  if (wantsB) {
    const info = rateLimitEvent('rejected', hitUsage, resetsInS, 'seven_day').rate_limit_info;
    out(
      assistantText(limitText, {
        error: 'rate_limit',
        is_api_error_message: true,
        api_error_status: 429,
        api_error: 'usage_limit_reached',
        api_error_params: { rate_limit_info: info },
        message: { role: 'assistant', model: '<synthetic>', content: [{ type: 'text', text: limitText }] },
      }),
    );
  } else {
    out(assistantText(turn.text || `가짜 claude 가 받았습니다: ${String(userText).slice(0, 60)}`));
  }

  if (turn.delay_ms) await sleep(turn.delay_ms);
  if (interrupted) return finishInterrupted();

  const writes = Array.isArray(turn.writes) ? turn.writes : [];
  writes.forEach((spec, i) => writeFile(spec, i + 1));

  const result = {
    type: 'result',
    subtype: 'success',
    is_error: false,
    duration_ms: Date.now() - startedAt,
    num_turns: turn.num_turns === undefined ? 1 + writes.length : turn.num_turns,
    result: turn.text || '끝냈습니다',
    total_cost_usd: turn.cost === undefined ? 0.01 : turn.cost,
    session_id: sessionId,
  };
  if (wantsB) result.result = limitText;
  if (wantsC) {
    Object.assign(result, { is_error: true, api_error_status: 429, api_error: 'usage_limit_reached', terminal_reason: 'api_error', result: limitText });
  }
  out(result);
  afterTurn(turn);
}

function afterTurn(turn) {
  if (turn.exit_code !== undefined) exitNow(turn.exit_code);
}

// ---- 입력 ----
const queue = [];
let inputClosed = false;

function userText(message) {
  const content = message && message.content;
  if (typeof content === 'string') return content;
  return Array.isArray(content) ? content.map((block) => (block && block.text) || '').join('') : '';
}

async function pump() {
  if (busy) return;
  busy = true;
  while (queue.length) {
    await runTurn(queue.shift());
    if (interrupted) return;
  }
  busy = false;
  if (interrupted) finishInterrupted();
  else if (inputClosed) exitNow(0);
}

function onLine(line) {
  record({ event: 'in', line });
  let parsed;
  try {
    parsed = JSON.parse(line);
  } catch (_) {
    return;
  }
  if (parsed.type === 'control_request') {
    const subtype = parsed.request && parsed.request.subtype;
    if (subtype === 'interrupt') onInterrupt(parsed.request_id);
    else out({ type: 'control_response', response: { subtype: 'success', request_id: parsed.request_id } });
  } else if (parsed.type === 'user') {
    queue.push(userText(parsed.message));
    pump();
  }
}

let pending = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', (chunk) => {
  pending += chunk;
  let at;
  while ((at = pending.indexOf('\n')) >= 0) {
    const line = pending.slice(0, at).replace(/\r$/, '');
    pending = pending.slice(at + 1);
    if (line.trim()) onLine(line);
  }
});
process.stdin.on('end', () => {
  if (pending.trim()) onLine(pending.replace(/\r$/, ''));
  inputClosed = true;
  if (!busy && !queue.length) exitNow(0);
});
process.stdin.resume();

process.on('SIGINT', () => {
  record({ event: 'signal', name: 'SIGINT' });
  onInterrupt(null);
});
process.on('SIGTERM', () => {
  record({ event: 'signal', name: 'SIGTERM' });
  exitNow(143);
});

setTimeout(() => {
  record({ event: 'timeout' });
  exitNow(1);
}, Number(process.env.FAKE_CLAUDE_MAX_MS) || 10 * 60 * 1000).unref();
