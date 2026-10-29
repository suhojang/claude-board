// 실제 어댑터: 하네스의 jevvision.cli 를 하위 프로세스로 부른다. HTTP·키 조회·캐시 형식은 전부 하네스 몫이고 여기에 없다.
//
// 키: 이 파일은 키를 읽지 않고, 로그에 쓰지 않고, 인자로 넘기지 않는다. 하위 프로세스가 환경을 그대로 물려받고
// 하네스 KeySource 가 환경변수·자격 증명 관리자에서 읽는다. 읽는 환경변수는 경로를 정하는 네 개(UI_AUTOMATION_HOME,
// UI_AUTOMATION_SCRIPTS, USERPROFILE, HOME)뿐이다.
// 파이썬: 하네스 venv(`<UI_AUTOMATION_HOME>/venv/Scripts/python.exe`)가 있으면 그것, 없으면 `python -I`.
// -I 는 PYTHONPATH 를 무시하므로 하네스 scripts 폴더는 짧은 부트스트랩이 sys.path 에 넣는다.
import cp from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { BUDGET, SCREENS, TIMEOUT_MS } from '../questions.mjs';
import { UsageError, normalizeItem, unverified } from '../port.mjs';
import { toScreenVerdict, toSelftestVerdict, toVerdicts } from '../domain/verdict.mjs';

const BOOTSTRAP =
  'import sys; sys.path.insert(0, sys.argv.pop(1)); from jevvision.cli import main; sys.exit(main(sys.argv[1:]))';

export function resolveCommand({ platform = process.platform, env = process.env, exists = fs.existsSync } = {}) {
  const userHome = env.USERPROFILE || env.HOME || os.homedir();
  const home = env.UI_AUTOMATION_HOME || path.join(userHome, '.local', 'share', 'ui-automation');
  const venvPython = platform === 'win32' ? path.join(home, 'venv', 'Scripts', 'python.exe') : path.join(home, 'venv', 'bin', 'python');
  const usedVenv = exists(venvPython);
  const scripts = env.UI_AUTOMATION_SCRIPTS || path.join(userHome, '.claude', 'skills', 'ui-automation', 'scripts');
  return {
    file: usedVenv ? venvPython : platform === 'win32' ? 'python' : 'python3',
    prefixArgs: ['-I', '-B', '-X', 'utf8', '-c', BOOTSTRAP, scripts],
    usedVenv,
  };
}

// 하위 프로세스 실행. 결과는 항상 값으로 돌려준다({rc, stdout} 또는 {error}) — 던지지 않는다.
export function defaultSpawn(file, args, { cwd, env, timeoutMs }) {
  return new Promise((resolve) => {
    let child;
    try {
      child = cp.spawn(file, args, { cwd, env, stdio: ['ignore', 'pipe', 'pipe'], windowsHide: true });
    } catch (error) {
      resolve({ error: error.code ?? 'spawn failed' });
      return;
    }
    const out = [];
    let timedOut = false;
    const timer = setTimeout(() => {
      timedOut = true;
      child.kill();
    }, timeoutMs);
    child.stdout.on('data', (d) => out.push(d));
    child.stderr.resume(); // 트레이스백 등 stderr 는 읽지도 남기지도 않는다
    child.on('error', (error) => {
      clearTimeout(timer);
      resolve({ error: error.code ?? 'spawn failed' });
    });
    child.on('close', (rc) => {
      clearTimeout(timer);
      resolve(timedOut ? { error: 'timeout' } : { rc, stdout: Buffer.concat(out).toString('utf8') });
    });
  });
}

// 판정 캐시 키에 들어간다. 이 어댑터가 하네스에 보내는 내용·읽는 방식이 달라지면 올린다.
export const JEVLOOK_VERSION = 1;

export class JevLookJudge {
  id = 'jevlook';
  version = JEVLOOK_VERSION;

  // command: {file, prefixArgs} (시험은 스텁 스크립트를 꽂는다). repo: 하네스 --repo (이미지는 <repo>/e2e/ 아래).
  constructor({ repo, cacheDir, command = resolveCommand(), budget = BUDGET, record = true, timeoutMs = TIMEOUT_MS, spawn = defaultSpawn, env = process.env } = {}) {
    if (!repo || !cacheDir) throw new TypeError('repo 와 cacheDir 이 필요하다');
    Object.assign(this, { repo, cacheDir, command, budget, record, timeoutMs, spawn, env });
    this.screenFiles = 0;
  }

  #common() {
    return ['--repo', this.repo, '--jev-cache', this.cacheDir, ...(this.record ? ['--record'] : []), '--budget', String(this.budget)];
  }

  async #run(args) {
    const result = await this.spawn(this.command.file, [...this.command.prefixArgs, ...args], {
      cwd: this.repo,
      env: this.env,
      timeoutMs: this.timeoutMs,
    });
    if (result.error) return { failure: unverified(result.error === 'timeout' ? 'jev timeout' : 'jevvision not runnable', String(result.error)) };
    return { rc: result.rc, lines: result.stdout.split('\n') };
  }

  async checkAll(imagePath, items) {
    const normalized = items.map((item, i) => normalizeItem(item, 'yes', i));
    const args = ['check', imagePath, ...normalized.flatMap((item) => ['--check', item.name, item.statement, item.expect]), ...this.#common()];
    const ran = await this.#run(args);
    if (ran.failure) return normalized.map(() => ran.failure);
    return toVerdicts(ran.rc, ran.lines, normalized.map((item) => item.name));
  }

  async check(imagePath, statement, expect = 'yes') {
    return (await this.checkAll(imagePath, [normalizeItem(statement, expect)]))[0];
  }

  async screen(imagePath, choices, expect) {
    for (const name of choices) {
      if (!/^[A-Za-z0-9_-]+$/.test(name) || name === 'other') throw new UsageError(`화면 이름이 잘못됐다: ${name}`);
    }
    if (choices.length === 0) throw new UsageError('choices 가 비었다');
    if (expect !== undefined && !choices.includes(expect)) throw new UsageError(`expect 가 choices 에 없다: ${expect}`);
    fs.mkdirSync(this.cacheDir, { recursive: true });
    const file = path.join(this.cacheDir, `screens-${process.pid}-${this.screenFiles++}.json`);
    fs.writeFileSync(file, JSON.stringify(Object.fromEntries(choices.map((name) => [name, SCREENS[name] ?? name]))), 'utf8');
    try {
      const ran = await this.#run(['screen', imagePath, '--screens', file, ...this.#common()]);
      return ran.failure ?? toScreenVerdict(ran.rc, ran.lines, expect);
    } finally {
      fs.rmSync(file, { force: true });
    }
  }

  async selftest() {
    const ran = await this.#run(['selftest', ...this.#common()]);
    return ran.failure ?? toSelftestVerdict(ran.rc, ran.lines);
  }
}
