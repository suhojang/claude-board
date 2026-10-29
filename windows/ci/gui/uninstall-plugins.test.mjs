// uninstall.ps1 의 plugins 지우기(#region plugins-copy)를 임시 폴더에서 시험한다 — 실제 설치·제거는 하지 않는다.
// 실행: node --test windows/ci/gui/uninstall-plugins.test.mjs   (Windows PowerShell 5.1 = powershell.exe 가 필요하다)
import { test } from 'node:test';
import assert from 'node:assert/strict';
import cp from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const SCRIPT = path.join(HERE, '..', '..', 'packaging', 'uninstall.ps1');
const SKIP = process.platform !== 'win32' || cp.spawnSync('powershell', ['-NoProfile', '-Command', '1']).status !== 0;

const source = fs.readFileSync(SCRIPT, 'utf8');
const region = source.slice(source.indexOf('#region plugins-copy'), source.indexOf('#endregion plugins-copy'));

function scratch() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'uninstall-plugins-'));
  return { dir, cleanup: () => fs.rmSync(dir, { recursive: true, force: true, maxRetries: 20, retryDelay: 50 }) };
}

function touch(file, text = 'x') {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, text);
}

// 정션은 권한 없이 만들 수 있다
function junction(link, target) {
  fs.mkdirSync(path.dirname(link), { recursive: true });
  const done = cp.spawnSync('cmd', ['/C', 'mklink', '/J', link, target], { encoding: 'utf8' });
  assert.equal(done.status, 0, done.stdout + done.stderr);
}

// region 을 임시 .ps1 에 담아 Windows PowerShell 5.1 로 돌린다(UTF-8 BOM — irm | iex 가 문자열을 UTF-8 로 읽는 것과 같게)
function runRemove(notifier, env = {}) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'uninstall-plugins-ps-'));
  try {
    const file = path.join(dir, 'run.ps1');
    fs.writeFileSync(file, `﻿${region}\nRemove-PluginsCopy '${notifier.replace(/'/g, "''")}'\n`, 'utf8');
    const merged = { ...process.env };
    delete merged.CLAUDE_BOARD_NOTIFIER_DIR;
    const done = cp.spawnSync('powershell', ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', file], { encoding: 'utf8', env: { ...merged, ...env } });
    return { code: done.status, text: `${done.stdout}${done.stderr}` };
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
}

test('plugins 를 통째로 지운다 — md-render 밖의 것과 Claude Code 가 만든 파일까지(macOS rm -rf plugins 와 같은 범위)', { skip: SKIP }, () => {
  const s = scratch();
  try {
    const notifier = path.join(s.dir, 'notifier');
    for (const f of ['md-render/LICENSE', 'md-render/hooks/register.tsx', 'md-render/tsconfig.json', 'md-render/.claude-plugin/types/index.d.ts', 'other/keep.txt']) {
      touch(path.join(notifier, 'plugins', f));
    }
    touch(path.join(notifier, 'sessions', 'a.json'));
    const r = runRemove(notifier);
    assert.equal(r.code, 0, r.text);
    assert.equal(fs.existsSync(path.join(notifier, 'plugins')), false, r.text);
    assert.equal(fs.existsSync(path.join(notifier, 'sessions', 'a.json')), true, 'plugins 밖은 이 단계가 건드리지 않는다');
  } finally {
    s.cleanup();
  }
});

test('CLAUDE_BOARD_NOTIFIER_DIR 로 옮긴 실행에서는 plugins 를 그대로 둔다', { skip: SKIP }, () => {
  const s = scratch();
  try {
    const notifier = path.join(s.dir, 'notifier');
    touch(path.join(notifier, 'plugins', 'md-render', 'LICENSE'));
    touch(path.join(notifier, 'plugins', 'other', 'keep.txt'));
    const r = runRemove(notifier, { CLAUDE_BOARD_NOTIFIER_DIR: notifier });
    assert.equal(r.code, 0, r.text);
    assert.equal(fs.existsSync(path.join(notifier, 'plugins', 'md-render', 'LICENSE')), true);
    assert.equal(fs.existsSync(path.join(notifier, 'plugins', 'other', 'keep.txt')), true);
  } finally {
    s.cleanup();
  }
});

test('링크는 따라가지 않는다 — md-render·그 하위 폴더·plugins 자체가 정션이어도 가리키는 곳의 파일은 남는다', { skip: SKIP }, () => {
  const s = scratch();
  try {
    // md-render 가 정션
    const a = path.join(s.dir, 'a');
    touch(path.join(s.dir, 'elsewhere-a', 'keep.txt'));
    junction(path.join(a, 'plugins', 'md-render'), path.join(s.dir, 'elsewhere-a'));
    let r = runRemove(a);
    assert.equal(r.code, 0, r.text);
    assert.equal(fs.existsSync(path.join(a, 'plugins')), false, r.text);
    assert.equal(fs.readFileSync(path.join(s.dir, 'elsewhere-a', 'keep.txt'), 'utf8'), 'x', 'md-render 정션 너머는 남는다');

    // md-render 안의 hooks 가 정션
    const b = path.join(s.dir, 'b');
    touch(path.join(s.dir, 'elsewhere-b', 'keep.txt'));
    touch(path.join(b, 'plugins', 'md-render', 'LICENSE'));
    junction(path.join(b, 'plugins', 'md-render', 'hooks'), path.join(s.dir, 'elsewhere-b'));
    r = runRemove(b);
    assert.equal(r.code, 0, r.text);
    assert.equal(fs.existsSync(path.join(b, 'plugins')), false, r.text);
    assert.equal(fs.readFileSync(path.join(s.dir, 'elsewhere-b', 'keep.txt'), 'utf8'), 'x', '하위 정션 너머는 남는다');

    // plugins 자체가 정션
    const c = path.join(s.dir, 'c');
    touch(path.join(s.dir, 'elsewhere-c', 'md-render', 'keep.txt'));
    fs.mkdirSync(c, { recursive: true });
    junction(path.join(c, 'plugins'), path.join(s.dir, 'elsewhere-c'));
    r = runRemove(c);
    assert.equal(r.code, 0, r.text);
    assert.equal(fs.existsSync(path.join(c, 'plugins')), false, r.text);
    assert.equal(fs.readFileSync(path.join(s.dir, 'elsewhere-c', 'md-render', 'keep.txt'), 'utf8'), 'x', 'plugins 정션 너머는 남는다');
  } finally {
    s.cleanup();
  }
});

test('plugins 가 없으면 아무 일도 없다', { skip: SKIP }, () => {
  const s = scratch();
  try {
    const r = runRemove(path.join(s.dir, 'none'));
    assert.equal(r.code, 0, r.text);
  } finally {
    s.cleanup();
  }
});

test('uninstall.ps1 규약 — Windows PowerShell 5.1 이 UTF-8 로 읽어 파싱한다, BOM·??·&&·exit 없음', { skip: SKIP }, () => {
  const bytes = fs.readFileSync(SCRIPT);
  assert.notDeepEqual([...bytes.subarray(0, 3)], [0xef, 0xbb, 0xbf], 'BOM 없음');
  const code = source.split('\n').filter((line) => !line.trimStart().startsWith('#')).join('\n');
  assert.doesNotMatch(code, /\?\?/);
  assert.doesNotMatch(code, /&&/);
  assert.doesNotMatch(code, /(^|[\s;{(])exit(\s|;|$)/m);
  const probe = `$text = [IO.File]::ReadAllText('${SCRIPT.replace(/'/g, "''")}', (New-Object Text.UTF8Encoding $false)); $errs = $null; [void][Management.Automation.Language.Parser]::ParseInput($text, [ref]$null, [ref]$errs); if ($errs.Count -gt 0) { $errs | ForEach-Object { $_.Message }; [Environment]::Exit(1) } else { 'PARSE-OK' }`;
  const done = cp.spawnSync('powershell', ['-NoProfile', '-Command', probe], { encoding: 'utf8' });
  assert.equal(done.status, 0, done.stdout + done.stderr);
  assert.match(done.stdout, /PARSE-OK/);
});
