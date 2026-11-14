'use strict';
// install.ps1·uninstall.ps1 이 떠 있는 보드를 어떻게 다루는지 시험한다(SPEC 5.8, gap/plan.md W0-4 · M-256 · C-327).
//   보드 탭 셸(CLAUDE_BOARD_TAB=1)에서는 보드를 끄지 않고, 보드 밖에서는 `ClaudeBoard.exe --action quit -` 으로 종료를 청해 확인 창을 거치고,
//   취소하거나 시간 안에 답이 없으면 설치·제거를 바꾼 것 없이 멈춘다. 보드의 업데이트 단추(-Update)는 보드가 이미 확인을 거쳤으므로 바로 끈다.
// 진짜 보드 대신 가짜 ClaudeBoard.exe(.NET Framework csc 로 만든 작은 exe)를 설치 폴더에 놓는다. 인자 없이 뜨면 보드 흉내(종료 표지 파일을 기다린다),
//   `--action …` 이면 받은 인자를 calls.log 에 남기고 시나리오(STUB_MODE)대로 %TEMP%\claude-board.quit 표지를 쓴다:
//   accept(asking 을 쓰고 보드가 정상 종료) · quick(확인 창 없이 바로 종료) · decline(asking 뒤 cancel) · ignore(asking 만) · legacy(요청을 모름, 아무것도 안 함).
// USERPROFILE·LOCALAPPDATA·TEMP 를 임시 폴더로 바꿔 돌리므로 이 PC 의 설치·설정은 건드리지 않는다. powershell.exe(5.1)나 csc.exe 가 없으면 건너뛴다.
// 실행: node --test windows/ci/install-quit.test.cjs
const { test } = require('node:test');
const assert = require('node:assert/strict');
const cp = require('node:child_process');
const crypto = require('node:crypto');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const zlib = require('node:zlib');

const WIN = path.resolve(__dirname, '..');
const FILES = {
  install: path.join(WIN, 'packaging', 'install.ps1'),
  uninstall: path.join(WIN, 'packaging', 'uninstall.ps1'),
  ps51: path.join(WIN, 'ci', 'ps51-install.ps1'),
};
const read = (file) => fs.readFileSync(file, 'utf8').replace(/^\uFEFF/, '').replace(/\r\n/g, '\n');

const SYSROOT = process.env.SystemRoot || 'C:\\Windows';
const POWERSHELL = process.platform === 'win32' ? path.join(SYSROOT, 'System32', 'WindowsPowerShell', 'v1.0', 'powershell.exe') : null;
const CSC = process.platform === 'win32' ? path.join(SYSROOT, 'Microsoft.NET', 'Framework64', 'v4.0.30319', 'csc.exe') : null;
const skip = !POWERSHELL || !fs.existsSync(POWERSHELL) ? 'Windows PowerShell 5.1(powershell.exe)이 없다' : !CSC || !fs.existsSync(CSC) ? '.NET Framework csc.exe 가 없다' : false;

// ───────── 정적 약속 ─────────

/** `#region quit-request` ~ `#endregion quit-request` 를 줄 앞 공백을 떼고 돌려준다 */
function quitRegion(text) {
  const m = text.match(/^[ \t]*#region quit-request\n([\s\S]*?)^[ \t]*#endregion quit-request$/m);
  assert.ok(m, '#region quit-request 를 찾지 못했다');
  return m[1].split('\n').map((l) => l.trim()).join('\n');
}

test('install.ps1 과 uninstall.ps1 의 종료 요청 구역(#region quit-request)은 글자까지 같다', () => {
  assert.equal(quitRegion(read(FILES.install)), quitRegion(read(FILES.uninstall)));
});

test('종료 요청은 `ClaudeBoard.exe --action quit -` 이고 표지 이름과 값이 W0-3 약속과 같다', () => {
  const region = quitRegion(read(FILES.install));
  assert.match(region, /-ArgumentList @\('--action', 'quit', '-'\)/);
  assert.match(region, /'claude-board\.quit'/);
  assert.match(region, /-eq 'cancel'/);
  assert.match(region, /CLAUDE_BOARD_TAB -eq '1'/);
});

test('두 스크립트는 BOM 이 없고 PowerShell 7 전용 문법·exit 를 쓰지 않는다', () => {
  for (const f of [FILES.install, FILES.uninstall]) {
    const buf = fs.readFileSync(f);
    assert.notDeepEqual([...buf.subarray(0, 3)], [0xef, 0xbb, 0xbf], `${f} 에 BOM`);
    const code = read(f).split('\n').filter((l) => !/^\s*#/.test(l)).map((l) => l.replace(/'[^']*'|"[^"]*"/g, "''")).join('\n');
    assert.doesNotMatch(code, /\?\?|&&|\|\||\bexit\b/, `${f} 에 5.1 에서 못 쓰는 문법이나 exit`);
    assert.doesNotMatch(code.replace(/#.*$/gm, ''), /[^\x00-\x7f]/, `${f} 의 코드에 ASCII 가 아닌 글자`);
  }
});

// ───────── 가짜 보드로 실제 실행 ─────────

const STUB_SRC = `using System;
using System.Diagnostics;
using System.IO;
using System.Threading;
class Stub {
  static void Main(string[] a) {
    string dir = Environment.GetEnvironmentVariable("STUB_DIR");
    string mode = Environment.GetEnvironmentVariable("STUB_MODE") ?? "";
    string ack = Path.Combine(Path.GetTempPath(), "claude-board.quit");
    string flag = Path.Combine(dir, "exit.flag");
    if (a.Length > 0 && a[0] == "--action") {
      File.AppendAllText(Path.Combine(dir, "calls.log"), string.Join(" ", a) + "\\n");
      if (mode == "accept") { File.WriteAllText(ack, "asking"); Thread.Sleep(300); File.WriteAllText(flag, "x"); }
      else if (mode == "quick") { File.WriteAllText(flag, "x"); }
      else if (mode == "decline") { File.WriteAllText(ack, "asking"); Thread.Sleep(300); File.WriteAllText(ack, "cancel"); }
      else if (mode == "ignore") { File.WriteAllText(ack, "asking"); }
      return;
    }
    File.WriteAllText(Path.Combine(dir, "board.pid"), Process.GetCurrentProcess().Id.ToString());
    for (int i = 0; i < 900; i++) {
      if (File.Exists(flag)) { File.WriteAllText(Path.Combine(dir, "exited-by-flag"), "x"); return; }
      Thread.Sleep(100);
    }
  }
}
`;

const EXES = ['ClaudeBoard.exe', 'claude-board-hook.exe', 'claude-autopilot.exe', 'claude-autopilot-engine.exe'];
const PLUGIN = ['LICENSE', 'README.md', '.claude-plugin/plugin.json', 'hooks/hooks.json', 'hooks/register.tsx'].map((f) => `plugins/md-render/${f}`);

/** 최소한의 zip(저장만, 압축 없음) */
function makeZip(entries) {
  const locals = [];
  const centrals = [];
  let offset = 0;
  for (const [name, content] of entries) {
    const nameBuf = Buffer.from(name, 'utf8');
    const data = Buffer.from(content, 'utf8');
    const crc = zlib.crc32(data) >>> 0;
    const local = Buffer.alloc(30);
    local.writeUInt32LE(0x04034b50, 0);
    local.writeUInt16LE(20, 4);
    local.writeUInt16LE(0x0800, 6);
    local.writeUInt32LE(0x00210000, 10);
    local.writeUInt32LE(crc, 14);
    local.writeUInt32LE(data.length, 18);
    local.writeUInt32LE(data.length, 22);
    local.writeUInt16LE(nameBuf.length, 26);
    locals.push(local, nameBuf, data);
    const central = Buffer.alloc(46);
    central.writeUInt32LE(0x02014b50, 0);
    central.writeUInt16LE(20, 4);
    central.writeUInt16LE(20, 6);
    central.writeUInt16LE(0x0800, 8);
    central.writeUInt32LE(0x00210000, 12);
    central.writeUInt32LE(crc, 16);
    central.writeUInt32LE(data.length, 20);
    central.writeUInt32LE(data.length, 24);
    central.writeUInt16LE(nameBuf.length, 28);
    central.writeUInt32LE(offset, 42);
    centrals.push(central, nameBuf);
    offset += 30 + nameBuf.length + data.length;
  }
  const centralBuf = Buffer.concat(centrals);
  const end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50, 0);
  end.writeUInt16LE(entries.length, 8);
  end.writeUInt16LE(entries.length, 10);
  end.writeUInt32LE(centralBuf.length, 12);
  end.writeUInt32LE(offset, 16);
  return Buffer.concat([...locals, centralBuf, end]);
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const alive = (pid) => {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
};

let stubExe = null;
function buildStub(root) {
  if (stubExe) return stubExe;
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'cb-quit-stubsrc-'));
  const src = path.join(dir, 'stub.cs');
  fs.writeFileSync(src, STUB_SRC);
  const out = path.join(dir, 'ClaudeBoard.exe');
  const r = cp.spawnSync(CSC, ['/nologo', '/target:exe', `/out:${out}`, src], { encoding: 'utf8' });
  assert.equal(r.status, 0, `csc 실패: ${r.stdout}${r.stderr}`);
  stubExe = out;
  return out;
}

/**
 * 한 시나리오. board=true 면 가짜 보드를 설치 폴더의 ClaudeBoard.exe 로 먼저 띄운다.
 * 돌려주는 값: outcome(ps51-install.ps1 결과 JSON), stdout, 보드 흉내의 상태(alive·byFlag), 요청 기록, 설치 폴더 파일 내용
 */
async function scenario({ script, mode, board, stub, tab = false, wait }) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'cb-quit-'));
  let pid = null;
  try {
    const assets = path.join(root, 'assets');
    fs.mkdirSync(assets);
    const zip = makeZip([...EXES, ...PLUGIN].map((n) => [n, `dummy ${n}`]));
    fs.writeFileSync(path.join(assets, 'ClaudeBoard-windows-x64.zip'), zip);
    const hash = crypto.createHash('sha256').update(zip).digest('hex');
    fs.writeFileSync(path.join(assets, 'ClaudeBoard-windows-x64.zip.sha256'), `${hash}  ClaudeBoard-windows-x64.zip\n`);

    const profile = path.join(root, 'profile');
    const local = path.join(root, 'local');
    const installDir = path.join(local, 'Programs', 'Claude Board');
    const stubDir = path.join(root, 'stub');
    fs.mkdirSync(installDir, { recursive: true });
    fs.mkdirSync(stubDir);
    fs.mkdirSync(path.join(profile, '.claude'), { recursive: true });
    fs.mkdirSync(path.join(root, 'tmp'));
    // 처음 설치가 아니게(hook 이 이미 있게) 해 확인 질문 없이 흐르게 한다
    fs.writeFileSync(path.join(profile, '.claude', 'settings.json'), '{"hooks":{"x":"claude-board-hook .claude/notifier/ x"}}');
    const exe = path.join(installDir, 'ClaudeBoard.exe');
    const stubBytes = fs.readFileSync(buildStub(root));
    fs.writeFileSync(exe, stubBytes);

    const env = { ...process.env, USERPROFILE: profile, LOCALAPPDATA: local, APPDATA: path.join(profile, 'AppData'), TEMP: path.join(root, 'tmp'), TMP: path.join(root, 'tmp'), STUB_DIR: stubDir, STUB_MODE: stub || '' };
    for (const key of Object.keys(env)) if (['psmodulepath', 'claude_board_tab', 'claude_board_install_quit_wait'].includes(key.toLowerCase())) delete env[key];
    if (tab) env.CLAUDE_BOARD_TAB = '1';
    if (wait) env.CLAUDE_BOARD_INSTALL_QUIT_WAIT = String(wait);

    if (board) {
      const child = cp.spawn(exe, [], { env, detached: true, stdio: 'ignore' });
      child.unref();
      const pidFile = path.join(stubDir, 'board.pid');
      for (let i = 0; i < 100 && !fs.existsSync(pidFile); i++) await sleep(100);
      assert.ok(fs.existsSync(pidFile), '가짜 보드가 뜨지 않았다');
      pid = Number(fs.readFileSync(pidFile, 'utf8'));
    }

    const result = path.join(root, 'result.json');
    const r = cp.spawnSync(
      POWERSHELL,
      ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', FILES.ps51, '-Mode', mode, '-Script', script, '-Assets', assets, '-CiVersion', '99.5', '-Result', result],
      { encoding: 'utf8', timeout: 150000, env },
    );
    assert.equal(r.status, 0, `ps51-install.ps1 자체가 실패했다: ${r.stderr}\n${r.stdout}`);
    await sleep(500);
    const calls = path.join(stubDir, 'calls.log');
    const files = fs.existsSync(installDir) ? fs.readdirSync(installDir) : [];
    return {
      outcome: JSON.parse(read(result)),
      stdout: r.stdout,
      boardAlive: pid === null ? null : alive(pid),
      byFlag: fs.existsSync(path.join(stubDir, 'exited-by-flag')),
      calls: fs.existsSync(calls) ? read(calls).trim().split('\n') : [],
      ackLeft: fs.existsSync(path.join(root, 'tmp', 'claude-board.quit')),
      installDirExists: fs.existsSync(installDir),
      exeIsStub: fs.existsSync(exe) && fs.readFileSync(exe).equals(stubBytes),
      exeText: fs.existsSync(exe) && !fs.readFileSync(exe).equals(stubBytes) ? fs.readFileSync(exe, 'utf8') : null,
      oldFiles: files.filter((f) => f.includes('.old-')),
    };
  } finally {
    if (pid !== null && alive(pid)) {
      try {
        process.kill(pid);
      } catch {
        /* 이미 끝남 */
      }
    }
    await sleep(200);
    fs.rmSync(root, { recursive: true, force: true });
  }
}

const NEW_EXE = 'dummy ClaudeBoard.exe';

test('(a) 보드가 안 떠 있으면 요청 없이 파일을 바꿔 끼운다', { skip, timeout: 180000 }, async () => {
  const r = await scenario({ script: FILES.install, mode: 'first', board: false });
  assert.deepEqual(r.calls, [], '떠 있지도 않은 보드에 요청을 보냈다');
  assert.equal(r.exeText, NEW_EXE, `파일을 바꿔 끼우지 못했다\n${r.stdout}`);
});

test('(b1) 보드가 떠 있고 끊길 세션이 없으면(확인 창 없이 종료) 요청 한 번으로 꺼지고 파일을 바꾼다', { skip, timeout: 180000 }, async () => {
  const r = await scenario({ script: FILES.install, mode: 'first', board: true, stub: 'quick' });
  assert.deepEqual(r.calls, ['--action quit -']);
  assert.equal(r.boardAlive, false);
  assert.equal(r.byFlag, true, '요청이 아니라 강제로 꺼졌다');
  assert.equal(r.exeText, NEW_EXE, r.stdout);
});

test('(b2) 요청을 모르는 옛 보드(표지도 종료도 없음)는 5초 뒤 지금처럼 끄고 파일을 바꾼다', { skip, timeout: 180000 }, async () => {
  const r = await scenario({ script: FILES.install, mode: 'first', board: true, stub: 'legacy' });
  assert.deepEqual(r.calls, ['--action quit -']);
  assert.equal(r.boardAlive, false, '옛 보드가 안 꺼졌다');
  assert.equal(r.byFlag, false, '요청으로 꺼진 것이 아니다(강제 종료여야 한다)');
  assert.equal(r.exeText, NEW_EXE, r.stdout);
});

test('(c1) 확인 창에서 계속하면(asking 뒤 보드가 정상 종료) 강제 종료 없이 파일을 바꾼다', { skip, timeout: 180000 }, async () => {
  const r = await scenario({ script: FILES.install, mode: 'first', board: true, stub: 'accept' });
  assert.deepEqual(r.calls, ['--action quit -']);
  assert.equal(r.boardAlive, false);
  assert.equal(r.byFlag, true, '보드가 요청으로 꺼지지 않고 강제로 꺼졌다');
  assert.equal(r.exeText, NEW_EXE, r.stdout);
  assert.equal(r.ackLeft, false, '표지를 지우지 않았다');
});

test('(c2) 확인 창에서 취소하면(cancel) 보드를 두고 아무것도 바꾸지 않고 멈춘다', { skip, timeout: 180000 }, async () => {
  const r = await scenario({ script: FILES.install, mode: 'first', board: true, stub: 'decline' });
  assert.deepEqual(r.calls, ['--action quit -']);
  assert.equal(r.outcome.threw, true, r.stdout);
  assert.match(r.outcome.error, /취소/, r.outcome.error);
  assert.equal(r.boardAlive, true, '취소했는데 보드가 꺼졌다');
  assert.equal(r.exeIsStub, true, '취소했는데 앱 파일이 바뀌었다');
  assert.deepEqual(r.oldFiles, [], '취소했는데 .old- 백업이 생겼다');
  assert.equal(r.ackLeft, false, '표지를 지우지 않았다');
});

test('(c3) 확인 창에 답이 없으면(asking 만, 기다린 시간 초과) 묻지 못하는 무인 실행은 보드를 두고 바꾼 것 없이 멈춘다', { skip, timeout: 180000 }, async () => {
  const r = await scenario({ script: FILES.install, mode: 'first', board: true, stub: 'ignore', wait: 2 });
  assert.deepEqual(r.calls, ['--action quit -']);
  assert.equal(r.outcome.threw, true, r.stdout);
  assert.match(r.outcome.error, /시간 안에 꺼지지 않았고/, r.outcome.error);
  assert.equal(r.boardAlive, true, '답이 없는데 보드가 꺼졌다');
  assert.equal(r.exeIsStub, true, '답이 없는데 앱 파일이 바뀌었다');
});

test('(d) 보드 탭 셸(CLAUDE_BOARD_TAB=1)에서 돌리면 요청도 종료도 없이 파일만 바꾸고 보드는 계속 돈다', { skip, timeout: 180000 }, async () => {
  const r = await scenario({ script: FILES.install, mode: 'first', board: true, stub: 'accept', tab: true });
  assert.deepEqual(r.calls, [], '보드 탭 안인데 종료를 청했다');
  assert.equal(r.boardAlive, true, '보드 탭 안에서 돌린 설치가 보드를 껐다');
  assert.equal(r.exeText, NEW_EXE, `파일을 바꿔 끼우지 못했다\n${r.stdout}`);
  assert.match(r.stdout, /보드 탭 안에서 실행 중이라 떠 있는 보드는 끄지 않고 파일만 바꿉니다/);
});

test('(e) 보드의 업데이트 단추(-Update)는 보드가 이미 확인을 거쳤으므로 요청 없이 끄고 바꾼다', { skip, timeout: 180000 }, async () => {
  const r = await scenario({ script: FILES.install, mode: 'update', board: true, stub: 'decline' });
  assert.deepEqual(r.calls, [], '이미 확인한 길인데 다시 종료를 청했다');
  assert.equal(r.boardAlive, false);
  assert.equal(r.exeText, NEW_EXE, r.stdout);
});

test('(f) uninstall.ps1: 보드 탭 셸에서는 바꾼 것 없이 멈추고 보드와 앱 폴더를 둔다', { skip, timeout: 180000 }, async () => {
  const r = await scenario({ script: FILES.uninstall, mode: 'uninstall', board: true, stub: 'accept', tab: true });
  assert.deepEqual(r.calls, []);
  assert.equal(r.outcome.threw, true, r.stdout);
  assert.match(r.outcome.error, /보드 탭/, r.outcome.error);
  assert.equal(r.boardAlive, true);
  assert.equal(r.exeIsStub, true, '앱 폴더를 건드렸다');
});

test('(g) uninstall.ps1: 보드 밖에서는 종료를 청하고 계속하면 앱 폴더를 지운다 / 취소하면 그대로 둔다', { skip, timeout: 180000 }, async () => {
  const yes = await scenario({ script: FILES.uninstall, mode: 'uninstall', board: true, stub: 'accept' });
  assert.deepEqual(yes.calls, ['--action quit -']);
  assert.equal(yes.byFlag, true, '요청이 아니라 강제로 꺼졌다');
  assert.equal(yes.installDirExists, false, yes.stdout);
  const no = await scenario({ script: FILES.uninstall, mode: 'uninstall', board: true, stub: 'decline' });
  assert.deepEqual(no.calls, ['--action quit -']);
  assert.equal(no.outcome.threw, true, no.stdout);
  assert.equal(no.boardAlive, true);
  assert.equal(no.exeIsStub, true, '취소했는데 앱 폴더를 건드렸다');
});
