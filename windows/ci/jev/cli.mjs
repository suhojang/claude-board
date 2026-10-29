// jev.mjs 의 본체. main(argv, deps) 는 종료코드를 돌려주고 출력은 deps.out/err 로 낸다(시험이 가짜를 꽂는 자리).
//
// 종료코드: 0 PASS · 1 FAIL · 2 사용법 · 3 UNSURE · 5 GUARD · 6 UNVERIFIED. 출력 첫 줄이 상태 낱말이라 UNVERIFIED 를 통과로 읽을 수 없다.
//   check    --image <png> --family <이름> [--require-calibrated] [--replay] [--verbose] [--allowed-window-title <창 제목>]
//   screen   --image <png> --choices a,b,c [--expect a] [--allowed-window-title <창 제목>]
//   crop     --in <png> --rect x,y,w,h --out <png> [--window-title <창 제목>]   (옆에 <png>.manifest.json 을 쓴다)
//   report   --rows <json>
//   measure  --dataset <폴더>
//   selftest
// 공통: --repo <폴더>(기본 windows/, 이미지는 <repo>/e2e/{out,artifacts}/ 아래만)
import fs from 'node:fs';
import path from 'node:path';
import { parseArgs } from 'node:util';
import { fileURLToPath } from 'node:url';
import { cachedJudge, VerdictCache } from './cache.mjs';
import { cropFile, parseRect } from './crop.mjs';
import { GuardError } from './domain/guard.mjs';
import { guardedJudge } from './adapters/guardedJudge.mjs';
import { JevLookJudge } from './adapters/jevlook.mjs';
import { loadDataset, measure, measureLines } from './measure.mjs';
import { EXIT, UsageError, exitCodeOf, STATUS, unverified } from './port.mjs';
import { FAMILIES, SCREENS } from './questions.mjs';
import { aggregate } from './report.mjs';

const HERE = path.dirname(fileURLToPath(import.meta.url));
export const DEFAULT_REPO = path.resolve(HERE, '..', '..'); // windows/


const OPTIONS = {
  image: { type: 'string' },
  family: { type: 'string' },
  choices: { type: 'string' },
  expect: { type: 'string' },
  repo: { type: 'string' },
  in: { type: 'string' },
  out: { type: 'string' },
  rect: { type: 'string' },
  'window-title': { type: 'string' },
  'allowed-window-title': { type: 'string' },
  rows: { type: 'string' },
  dataset: { type: 'string' },
  budget: { type: 'string' },
  replay: { type: 'boolean' },
  verbose: { type: 'boolean' },
  'require-calibrated': { type: 'boolean' },
};

function printVerdict(verdict, deps, verbose) {
  deps.out(`${verdict.status}|${verdict.reason}`);
  if (verbose && verdict.detail) deps.out(`DETAIL|${verdict.detail}`);
  if (verdict.noul !== undefined) deps.err(`NOUL|${verdict.noul.toFixed(2)}`);
}

// 실제 어댑터를 데코레이터로 감싼다: 가드 -> 판정 캐시 -> 하위 프로세스. deps.judge 가 있으면(시험) 그것을 안쪽에 쓴다.
function buildJudge(options, deps, repo) {
  const cacheDir = path.join(repo, 'e2e', 'out', 'jev-cache');
  const inner =
    deps.judge ??
    new JevLookJudge({
      repo,
      cacheDir: path.join(cacheDir, 'harness'),
      record: !options.replay,
      ...(options.budget ? { budget: Number(options.budget) } : {}),
      ...(deps.env ? { env: deps.env } : {}),
    });
  const allowedWindowTitle = options['allowed-window-title'];
  return guardedJudge(cachedJudge(inner, new VerdictCache(cacheDir)), { repo, ...(allowedWindowTitle !== undefined ? { allowedWindowTitle } : {}), ...(deps.userName !== undefined ? { userName: deps.userName } : {}) });
}

async function runCheck(options, deps, repo) {
  if (!options.image || !options.family) throw new UsageError('check 는 --image 와 --family 가 필요하다');
  const family = FAMILIES[options.family];
  if (!family) throw new UsageError(`알 수 없는 가족: ${options.family} (${Object.keys(FAMILIES).join(', ')})`);
  const calibrated = family.calibration.status === 'calibrated';
  if (!calibrated) {
    if (options['require-calibrated']) {
      printVerdict(unverified('family not calibrated'), deps);
      return EXIT.UNVERIFIED;
    }
    deps.err(`NOTE|family ${options.family} is ${family.calibration.status}: advisory only`);
  }
  const verdict = await buildJudge(options, deps, repo).check(options.image, family.statement, family.expect);
  printVerdict(verdict, deps, options.verbose);
  return exitCodeOf(verdict.status);
}

async function runScreen(options, deps, repo) {
  if (!options.image || !options.choices) throw new UsageError('screen 은 --image 와 --choices 가 필요하다');
  const choices = options.choices.split(',').map((c) => c.trim()).filter(Boolean);
  const unknown = choices.filter((c) => !SCREENS[c]);
  if (unknown.length > 0) deps.err(`NOTE|choices without a description in questions.mjs: ${unknown.join(',')}`);
  const verdict = await buildJudge(options, deps, repo).screen(options.image, choices, options.expect);
  printVerdict(verdict, deps, options.verbose);
  if (verdict.choice) deps.out(`CHOICE|${verdict.choice}`);
  return exitCodeOf(verdict.status);
}

async function runMeasure(options, deps, repo) {
  if (!options.dataset) throw new UsageError('measure 는 --dataset 이 필요하다');
  const items = loadDataset(path.resolve(options.dataset));
  const result = await measure({ judge: buildJudge(options, deps, repo), items });
  if (result.verdict) {
    printVerdict(result.verdict, deps, true);
    return EXIT.UNVERIFIED;
  }
  for (const line of measureLines(result.families)) deps.out(line);
  return EXIT.PASS; // 측정이 끝났다는 뜻이다. 가족별 trust= 는 사람이 읽는다
}

async function runSelftest(options, deps, repo) {
  const judge = buildJudge(options, deps, repo);
  const verdict = await judge.selftest();
  deps.out(verdict.status === STATUS.PASS ? 'SELFTEST|PASS' : `${verdict.status}|${verdict.reason}`);
  if (options.verbose && verdict.detail) deps.out(`DETAIL|${verdict.detail}`);
  return exitCodeOf(verdict.status);
}

function runCrop(options, deps, repo) {
  if (!options.in || !options.out || !options.rect) throw new UsageError('crop 은 --in, --rect, --out 이 필요하다');
  const done = cropFile({ input: options.in, rect: parseRect(options.rect), output: options.out, repo, windowTitle: options['window-title'] });
  deps.out(`CROP|${done.path}|${done.width}x${done.height}`);
  return EXIT.PASS;
}

function runReport(options, deps) {
  if (!options.rows) throw new UsageError('report 는 --rows 가 필요하다');
  let rows;
  try {
    rows = JSON.parse(fs.readFileSync(options.rows, 'utf8'));
  } catch (error) {
    throw new UsageError(`rows 를 읽을 수 없다: ${options.rows} (${error.code ?? error.message})`);
  }
  if (!Array.isArray(rows)) throw new UsageError('rows 는 배열이어야 한다');
  let report;
  try {
    report = aggregate(rows);
  } catch (error) {
    throw new UsageError(error.message);
  }
  for (const line of report.lines) deps.out(line);
  return report.exitCode;
}

export async function main(argv, deps = {}) {
  const io = { out: (line) => console.log(line), err: (line) => console.error(line), ...deps };
  const [verb, ...rest] = argv;
  try {
    const { values } = parseArgs({ args: rest, options: OPTIONS, strict: true, allowPositionals: false });
    const repo = path.resolve(values.repo ?? DEFAULT_REPO);
    const verbs = { check: runCheck, screen: runScreen, crop: runCrop, report: runReport, measure: runMeasure, selftest: runSelftest };
    if (!verbs[verb]) throw new UsageError(`동사는 ${Object.keys(verbs).join('|')} 중 하나다`);
    return await verbs[verb](values, io, repo);
  } catch (error) {
    if (error instanceof GuardError) {
      io.out(`GUARD|${error.message}`);
      return EXIT.GUARD;
    }
    if (error instanceof UsageError || error?.code === 'ERR_PARSE_ARGS_UNKNOWN_OPTION' || error?.code === 'ERR_PARSE_ARGS_INVALID_OPTION_VALUE') {
      io.out(`USAGE|${error.message}`);
      return EXIT.USAGE;
    }
    throw error;
  }
}
