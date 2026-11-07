'use strict';
// 배포물 약속 시험 — CI 없이 로컬에서 돈다.
//   1) 같은 exe 묶음을 여러 파일이 따로 적는다. 하나만 고치면 CI 의 릴리스 확인이나 사용자 PC 에서야 드러나므로 파일을 읽어 맞춰 본다:
//        Cargo [[bin]] 이름 · tauri.windows.conf.json externalBin · .github/workflows/windows.yml (sidecar 복사·zip 담기·want_zip) ·
//        windows/packaging/install.ps1 $needs · windows/ci/install-checks.ps1 의 설치 폴더 확인 목록
//      어긋나면 어느 파일이 무엇을 빼먹었는지 그 파일 이름을 넣어 알린다.
//   2) install.ps1 이 자율주행 exe 둘 중 하나라도 없는 zip 을 받으면 설치 폴더를 건드리기 전에 거절한다(Windows PowerShell 5.1 에서 실제로 돌린다).
//      USERPROFILE·LOCALAPPDATA·TEMP 를 임시 폴더로 바꿔 돌리므로 이 PC 의 설치·설정은 건드리지 않는다. powershell.exe 가 없으면 이 부분만 건너뛴다.
// 실행: node --test windows/ci/packaging-contract.test.cjs
const { test } = require('node:test');
const assert = require('node:assert/strict');
const cp = require('node:child_process');
const crypto = require('node:crypto');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const zlib = require('node:zlib');

const WIN = path.resolve(__dirname, '..');
const REPO = path.resolve(WIN, '..');
const FILES = {
  workflow: path.join(REPO, '.github', 'workflows', 'windows.yml'),
  install: path.join(WIN, 'packaging', 'install.ps1'),
  tauriConf: path.join(WIN, 'app', 'src-tauri', 'tauri.windows.conf.json'),
  installChecks: path.join(WIN, 'ci', 'install-checks.ps1'),
  ps51: path.join(WIN, 'ci', 'ps51-install.ps1'),
  appCargo: path.join(WIN, 'app', 'src-tauri', 'Cargo.toml'),
};
// BOM \uACFC CRLF \uB97C \uC5C6\uC564\uB2E4(\uC6CC\uD06C\uD50C\uB85C \uD30C\uC77C\uC774 CRLF \uB85C \uC800\uC7A5\uB3FC \uC788\uC744 \uC218 \uC788\uB2E4)
const read = (file) => fs.readFileSync(file, 'utf8').replace(/^\uFEFF/, '').replace(/\r\n/g, '\n');

// ───────── 파일에서 exe 이름을 뽑는 함수들(글을 받아 뽑으므로 시험이 글을 바꿔 넣어 볼 수 있다) ─────────

/** Cargo.toml 의 [[bin]] name 들 */
function cargoBins(text) {
  return [...text.matchAll(/\[\[bin\]\][^\[]*?\bname\s*=\s*"([^"]+)"/g)].map((m) => m[1]);
}

/** tauri.windows.conf.json 의 bundle.externalBin 이름들(binaries/ 접두를 뗀다) */
function externalBins(text) {
  const list = JSON.parse(text).bundle.externalBin;
  assert.ok(Array.isArray(list), 'bundle.externalBin 이 배열이 아니다');
  return list.map((p) => path.posix.basename(p));
}

/** 워크플로의 `for n in a b c; do` (sidecar 자리에 복사) */
function workflowSidecarLoop(text) {
  const m = text.match(/for n in ([^;\n]+); do/);
  assert.ok(m, 'windows.yml 에서 sidecar 복사 반복문(for n in ...; do)을 찾지 못했다');
  return m[1].trim().split(/\s+/);
}

/** 워크플로의 zip 담기 — `Copy-Item (Join-Path $ws '<경로>') $pkg` 중 exe. [{name, dir}] */
function workflowPackaged(text) {
  return [...text.matchAll(/Copy-Item \(Join-Path \$ws '([^']+\.exe)'\) \$pkg/g)].map((m) => {
    const parts = m[1].split('\\');
    return { name: parts[parts.length - 1], dir: parts.slice(0, -1).join('\\') };
  });
}

/** 워크플로 want_zip 의 모든 항목 */
function workflowWantZip(text) {
  const m = text.match(/want_zip="\$\(printf '%s\\n' ([\s\S]*?) \| LC_ALL=C sort\)"/);
  assert.ok(m, 'windows.yml 에서 want_zip 을 찾지 못했다');
  return m[1].replace(/\\\n/g, ' ').trim().split(/\s+/);
}

/** install.ps1 의 `$needs = @(...)` 문자열들 */
function installNeeds(text) {
  const m = text.match(/\$needs\s*=\s*@\(([^)]*)\)/);
  assert.ok(m, 'install.ps1 에서 $needs 를 찾지 못했다');
  return [...m[1].matchAll(/'([^']+)'/g)].map((x) => x[1]);
}

/** install.ps1 의 $PluginFiles (zip 안 plugins\md-render\ 아래 상대 경로) */
function installPluginFiles(text) {
  const m = text.match(/\$PluginFiles\s*=\s*@\(([^)]*)\)/);
  assert.ok(m, 'install.ps1 에서 $PluginFiles 를 찾지 못했다');
  return [...m[1].matchAll(/'([^']+)'/g)].map((x) => x[1]);
}

/** install-checks.ps1 시나리오 1 의 설치 폴더 확인 목록(uninstall.exe 는 setup.exe 만 만든다) */
function installChecksList(text) {
  const m = text.match(/foreach \(\$f in @\(([^)]*'ClaudeBoard\.exe'[^)]*)\)\)/);
  assert.ok(m, 'install-checks.ps1 에서 설치 폴더 확인 목록을 찾지 못했다');
  return [...m[1].matchAll(/'([^']+)'/g)].map((x) => x[1]).filter((n) => n !== 'uninstall.exe');
}

const sorted = (xs) => [...new Set(xs)].sort();

/** 두 묶음이 같아야 한다. 다르면 파일 이름과 빠진/더 있는 것을 넣어 던진다 */
function assertSameSet(file, what, got, want) {
  const g = sorted(got);
  const w = sorted(want);
  const missing = w.filter((x) => !g.includes(x));
  const extra = g.filter((x) => !w.includes(x));
  assert.ok(
    missing.length === 0 && extra.length === 0,
    `${path.relative(REPO, file)}: ${what} 이(가) 다른 파일과 어긋난다 - 빠짐 [${missing.join(', ')}], 더 있음 [${extra.join(', ')}]`,
  );
}

// ───────── 기준: Cargo 가 만드는 exe ─────────

function cratesBins() {
  const crates = path.join(WIN, 'crates');
  const out = [];
  for (const dir of fs.readdirSync(crates)) {
    const toml = path.join(crates, dir, 'Cargo.toml');
    if (fs.existsSync(toml)) out.push(...cargoBins(read(toml)));
  }
  return out;
}

const appBins = () => cargoBins(read(FILES.appCargo));

/** 릴리스 zip·설치 폴더에 있어야 하는 exe 전부: 보드 + 크레이트가 만드는 sidecar */
const allExes = () => sorted([...appBins(), ...cratesBins()].map((n) => `${n}.exe`));
const sidecars = () => sorted(cratesBins());

test('기준이 비어 있지 않고 자율주행 두 exe 를 포함한다(파서가 조용히 0개를 읽지 않게)', () => {
  assert.deepEqual(appBins(), ['ClaudeBoard']);
  assert.ok(sidecars().includes('claude-autopilot') && sidecars().includes('claude-autopilot-engine') && sidecars().includes('claude-board-hook'), sidecars().join(','));
});

test('tauri.windows.conf.json externalBin = 크레이트가 만드는 sidecar 전부', () => {
  assertSameSet(FILES.tauriConf, 'externalBin', externalBins(read(FILES.tauriConf)), sidecars());
});

test('windows.yml: sidecar 자리에 복사하는 이름 = sidecar 전부', () => {
  assertSameSet(FILES.workflow, 'sidecar 복사 반복문', workflowSidecarLoop(read(FILES.workflow)), sidecars());
});

test('windows.yml: zip 에 담는 exe = 보드 + sidecar 전부, 그리고 제자리에서 가져온다', () => {
  const packaged = workflowPackaged(read(FILES.workflow));
  assertSameSet(FILES.workflow, 'zip 에 담는 exe', packaged.map((p) => p.name), allExes());
  for (const p of packaged) {
    const want = p.name === 'ClaudeBoard.exe' ? 'windows\\app\\src-tauri\\target\\release' : 'windows\\target\\release';
    assert.equal(p.dir, want, `windows.yml: ${p.name} 을(를) 빌드 결과가 없는 폴더에서 가져온다`);
  }
});

test('windows.yml want_zip 의 exe = 보드 + sidecar 전부, 플러그인 파일 = install.ps1 $PluginFiles', () => {
  const want = workflowWantZip(read(FILES.workflow));
  assertSameSet(FILES.workflow, 'want_zip 의 exe', want.filter((n) => n.endsWith('.exe')), allExes());
  const plugin = want.filter((n) => n.startsWith('plugins/md-render/')).map((n) => n.slice('plugins/md-render/'.length));
  const fromInstall = installPluginFiles(read(FILES.install)).map((f) => f.replace(/\\/g, '/'));
  assertSameSet(FILES.workflow, 'want_zip 의 플러그인 파일', plugin, fromInstall);
});

test('install.ps1 $needs 의 exe = 보드 + sidecar 전부', () => {
  assertSameSet(FILES.install, '$needs 의 exe', installNeeds(read(FILES.install)).filter((n) => n.endsWith('.exe')), allExes());
});

test('install-checks.ps1 설치 폴더 확인 목록 = 보드 + sidecar 전부', () => {
  assertSameSet(FILES.installChecks, '설치 폴더 확인 목록', installChecksList(read(FILES.installChecks)), allExes());
});

// ───────── 약속 시험이 실제로 어긋남을 잡는지(글을 바꿔 넣어 본다) ─────────

test('파서와 비교가 어긋남을 잡고 파일 이름을 댄다', () => {
  const want = allExes();
  const install = read(FILES.install);
  // install.ps1 에서 엔진 exe 를 뺀다
  const noEngine = install.replace(", 'claude-autopilot-engine.exe'", '');
  assert.ok(noEngine !== install, '시험 전제: 글에 엔진 exe 가 있다');
  assert.throws(() => assertSameSet(FILES.install, '$needs', installNeeds(noEngine), want), /install\.ps1.*claude-autopilot-engine\.exe/);
  // 워크플로 want_zip 에서 명령줄 exe 를 뺀다
  const wf = read(FILES.workflow);
  const noCli = wf.replace('claude-autopilot.exe claude-autopilot-engine.exe', 'claude-autopilot-engine.exe');
  assert.ok(noCli !== wf, '시험 전제: want_zip 에 두 exe 가 같이 있다');
  assert.throws(() => assertSameSet(FILES.workflow, 'want_zip', workflowWantZip(noCli).filter((n) => n.endsWith('.exe')), want), /windows\.yml.*claude-autopilot\.exe/);
  // 워크플로 sidecar 반복문에서 엔진을 뺀다
  const noLoop = wf.replace('claude-autopilot claude-autopilot-engine; do', 'claude-autopilot; do');
  assert.ok(noLoop !== wf, '시험 전제: sidecar 반복문에 엔진이 있다');
  assert.throws(() => assertSameSet(FILES.workflow, 'loop', workflowSidecarLoop(noLoop), sidecars()), /claude-autopilot-engine/);
  // externalBin 에서 하나를 뺀다
  const conf = JSON.parse(read(FILES.tauriConf));
  conf.bundle.externalBin = conf.bundle.externalBin.filter((p) => !p.endsWith('claude-autopilot'));
  assert.throws(() => assertSameSet(FILES.tauriConf, 'externalBin', externalBins(JSON.stringify(conf)), sidecars()), /tauri\.windows\.conf\.json.*claude-autopilot\b/);
  // 새 [[bin]] 이 생기면 기준이 따라 늘어 다른 파일들이 어긋난다
  const withNew = cargoBins('[[bin]]\nname = "new-tool"\npath = "x.rs"\n[[bin]]\nname = "other"\n');
  assert.deepEqual(withNew, ['new-tool', 'other']);
});

// ───────── install.ps1 이 자율주행 exe 가 빠진 zip 을 거절하는지(Windows PowerShell 5.1 에서 실제로) ─────────

const POWERSHELL = process.platform === 'win32' ? path.join(process.env.SystemRoot || 'C:\\Windows', 'System32', 'WindowsPowerShell', 'v1.0', 'powershell.exe') : null;
const hasPs51 = POWERSHELL !== null && fs.existsSync(POWERSHELL);

/** 최소한의 zip(저장만, 압축 없음). 항목 이름은 `/` 구분자 */
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
    local.writeUInt16LE(20, 4); // 풀 때 필요한 버전
    local.writeUInt16LE(0x0800, 6); // UTF-8 이름
    local.writeUInt16LE(0, 8); // 저장
    local.writeUInt32LE(0x00210000, 10); // 시각(고정)
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
    central.writeUInt16LE(0, 10);
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

const PLUGIN_ENTRIES = ['LICENSE', 'README.md', '.claude-plugin/plugin.json', 'hooks/hooks.json', 'hooks/register.tsx'].map((f) => `plugins/md-render/${f}`);
const FULL_ENTRIES = [...allExes(), 'LICENSE', 'README.md', 'THIRD_PARTY_NOTICES.txt', ...PLUGIN_ENTRIES];

/** zip 하나로 install.ps1 을 5.1 에서 돌린다. 설치 폴더(이 PC 의 것이 아닌 임시 폴더)에 먼저 놓인 파일이 그대로인지도 돌려준다 */
function runInstall(names) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'cb-pkg-contract-'));
  try {
    const assets = path.join(root, 'assets');
    fs.mkdirSync(assets);
    const zip = makeZip(names.map((n) => [n, `dummy ${n}`]));
    fs.writeFileSync(path.join(assets, 'ClaudeBoard-windows-x64.zip'), zip);
    const hash = crypto.createHash('sha256').update(zip).digest('hex');
    fs.writeFileSync(path.join(assets, 'ClaudeBoard-windows-x64.zip.sha256'), `${hash}  ClaudeBoard-windows-x64.zip\n`);

    const profile = path.join(root, 'profile');
    const local = path.join(root, 'local');
    const installDir = path.join(local, 'Programs', 'Claude Board');
    fs.mkdirSync(installDir, { recursive: true });
    fs.mkdirSync(path.join(profile, '.claude'), { recursive: true });
    const sentinel = path.join(installDir, 'ClaudeBoard.exe');
    fs.writeFileSync(sentinel, 'OLD-INSTALL');
    fs.mkdirSync(path.join(root, 'tmp'));

    const result = path.join(root, 'result.json');
    // 부모가 PowerShell 7 이면 PSModulePath 에 7 의 모듈 폴더가 들어 있어 5.1 이 Get-FileHash 같은 기본 cmdlet 을 못 불러온다 - 지워서 5.1 이 제 기본값을 쓰게 한다
    const env = { ...process.env, USERPROFILE: profile, LOCALAPPDATA: local, APPDATA: path.join(profile, 'AppData'), TEMP: path.join(root, 'tmp'), TMP: path.join(root, 'tmp') };
    // (스프레드로 복사한 객체는 대소문자를 가리므로 이름을 모두 훑는다)
    for (const key of Object.keys(env)) if (key.toLowerCase() === 'psmodulepath') delete env[key];
    const r = cp.spawnSync(
      POWERSHELL,
      ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', FILES.ps51, '-Mode', 'update', '-Script', FILES.install, '-Assets', assets, '-CiVersion', '99.5', '-Result', result],
      { encoding: 'utf8', timeout: 120000, env },
    );
    assert.equal(r.status, 0, `ps51-install.ps1 자체가 실패했다: ${r.stderr}\n${r.stdout}`);
    const outcome = JSON.parse(read(result));
    return {
      outcome,
      sentinel: fs.readFileSync(sentinel, 'utf8'),
      installDirFiles: fs.readdirSync(installDir).sort(),
      settingsCreated: fs.existsSync(path.join(profile, '.claude', 'settings.json')),
      notifierCreated: fs.existsSync(path.join(profile, '.claude', 'notifier')),
      stdout: r.stdout,
    };
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
}

const skipPs = hasPs51 ? false : 'Windows PowerShell 5.1(powershell.exe)이 없다';

// [빠진 항목, 오류 글이 이름을 대야 하는 파일]
const MISSING = [
  ['claude-autopilot.exe', /claude-autopilot\.exe/],
  ['claude-autopilot-engine.exe', /claude-autopilot-engine\.exe/],
  ['claude-board-hook.exe', /claude-board-hook\.exe/],
  ['ClaudeBoard.exe', /ClaudeBoard\.exe/],
];

for (const [missing, pattern] of MISSING) {
  test(`install.ps1 은 ${missing} 가 빠진 zip 을 설치 폴더를 건드리기 전에 거절한다`, { skip: skipPs }, () => {
    assert.ok(FULL_ENTRIES.includes(missing), `시험 전제: 완전한 zip 목록에 ${missing} 가 있다`);
    const r = runInstall(FULL_ENTRIES.filter((n) => n !== missing));
    assert.equal(r.outcome.threw, true, `거절하지 않고 지나갔다\n${r.stdout}`);
    assert.match(r.outcome.error, /받은 zip 에 .* 가 없습니다/, r.outcome.error);
    assert.match(r.outcome.error, pattern, r.outcome.error);
    assert.equal(r.sentinel, 'OLD-INSTALL', '먼저 있던 설치 파일이 바뀌었다');
    assert.deepEqual(r.installDirFiles, ['ClaudeBoard.exe'], '설치 폴더에 파일이 생겼다(.old- 백업 포함)');
    assert.equal(r.settingsCreated, false, 'Claude Code 설정을 건드렸다');
    assert.equal(r.notifierCreated, false, 'notifier 폴더를 만들었다');
  });
}

test('자율주행 exe 둘이 다 있고 플러그인 파일 하나만 빠진 zip 은 그 파일 이름으로 거절한다(exe 확인이 멀쩡한 zip 을 막지 않는다)', { skip: skipPs }, () => {
  const r = runInstall(FULL_ENTRIES.filter((n) => n !== 'plugins/md-render/hooks/hooks.json'));
  assert.equal(r.outcome.threw, true);
  assert.match(r.outcome.error, /hooks\\hooks\.json/, r.outcome.error);
  assert.doesNotMatch(r.outcome.error, /autopilot/, r.outcome.error);
  assert.equal(r.sentinel, 'OLD-INSTALL');
});
