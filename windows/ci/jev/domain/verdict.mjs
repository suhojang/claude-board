// jevvision.cli 의 종료코드와 출력 줄 -> 판정. 순수 함수(프로세스·파일을 모른다).
//
// 규칙: PASS/FAIL/UNSURE 는 종료코드와 결과 줄이 둘 다 맞아야 나온다.
// 종료코드만 0 이고 줄이 없거나(파이썬이 줄을 못 냈다), 종료코드 1 이 CHECK FAIL 줄 없이 나오는 것(파이썬 예외도 1 이다)은
// 모두 UNVERIFIED 다. 비-0 종료코드는 어떤 경로로도 PASS 가 되지 않는다.
import { STATUS, unverified } from '../port.mjs';

// 어댑터가 읽고 남기는 줄의 머리. 이 밖의 줄(트레이스백 등)은 버린다. 키가 섞일 길을 줄인다.
export const KEPT_TAGS = Object.freeze(['CHECK', 'SCREEN', 'ASK', 'ERROR', 'LEDGER', 'JEV', 'SELFTEST']);

const KEY_NAMED = /TYPESAFE_API_KEY|typesafe-api-key|Jev 키/;

export function parseLines(lines) {
  const kept = [];
  for (const raw of lines) {
    const fields = String(raw).replace(/\r$/, '').split('|');
    if (KEPT_TAGS.includes(fields[0])) kept.push({ tag: fields[0], fields, text: fields.join('|') });
  }
  return kept;
}

// 토큰처럼 긴 글자 덩어리와 줄바꿈을 지우고 자른다. 상세(detail)에만 쓴다.
export function sanitizeDetail(text) {
  return String(text)
    .replace(/[A-Za-z0-9_-]{32,}/g, '[redacted]')
    .replace(/\s+/g, ' ')
    .trim()
    .slice(0, 200);
}

function noulOf(fields) {
  const found = fields.find((f) => f.startsWith('noul='));
  const value = found ? Number(found.slice(5)) : NaN;
  return Number.isFinite(value) ? value : undefined;
}

// 결과 줄이 없는 종료코드의 UNVERIFIED. 이유는 짧은 고정 문구, 줄 내용은 detail 에만 둔다.
function nonResult(rc, parsed) {
  const error = parsed.find((l) => l.tag === 'ERROR');
  const detail = error ? sanitizeDetail(error.fields.slice(3).join('|') || error.text) : undefined;
  if (rc === 6) {
    return unverified(error && KEY_NAMED.test(error.text) ? 'no Jev key' : 'jev precondition', detail);
  }
  if (rc === 7) return unverified('jev error or cache miss', detail);
  if (rc === 2) return unverified('jev usage error', detail);
  if (rc === 4) return unverified('jev budget used up', detail);
  if (rc === 5) return unverified('jev guard refused', detail);
  return unverified(`jevvision exit ${rc}`, detail);
}

function askOf(parsed) {
  const ask = parsed.find((l) => l.tag === 'ASK');
  return ask ? sanitizeDetail(ask.fields.slice(2).join('|')) : undefined;
}

// check 동사: names 순서대로 판정 하나씩.
export function toVerdicts(rc, lines, names) {
  const parsed = parseLines(lines);
  if (![0, 1, 3].includes(rc)) return names.map(() => nonResult(rc, parsed));
  const results = new Map();
  for (const line of parsed) if (line.tag === 'CHECK') results.set(line.fields[2], line);
  const statuses = names.map((name) => results.get(name)?.fields[1]);
  const known = statuses.every((s) => s === 'PASS' || s === 'FAIL' || s === 'UNSURE');
  const ask = askOf(parsed);
  if (rc === 3 && names.every((n) => !results.has(n)) && ask !== undefined) {
    return names.map(() => ({ status: STATUS.UNSURE, reason: 'jev asked for a human', detail: ask }));
  }
  const consistent =
    known &&
    (rc === 0
      ? statuses.every((s) => s === 'PASS')
      : rc === 1
        ? statuses.includes('FAIL')
        : !statuses.includes('FAIL') && statuses.includes('UNSURE'));
  if (!consistent) {
    return names.map(() => unverified('exit code and result lines disagree', `rc=${rc} lines=${parsed.length}`));
  }
  return names.map((name, i) => {
    const verdict = { status: statuses[i], reason: `jev ${statuses[i].toLowerCase()}` };
    const noul = noulOf(results.get(name).fields);
    if (noul !== undefined) verdict.noul = noul;
    return verdict;
  });
}

export function toVerdict(rc, lines, name = 'q1') {
  return toVerdicts(rc, lines, [name])[0];
}

// screen 동사. expect 가 있으면 고른 화면이 같을 때만 PASS, 다르면 FAIL. other 는 UNSURE.
export function toScreenVerdict(rc, lines, expect) {
  const parsed = parseLines(lines);
  if (rc === 3) {
    const ask = askOf(parsed);
    return ask === undefined
      ? unverified('exit code and result lines disagree', 'rc=3 without ASK line')
      : { status: STATUS.UNSURE, reason: 'jev asked for a human', detail: ask };
  }
  if (rc !== 0) return nonResult(rc, parsed);
  const line = parsed.find((l) => l.tag === 'SCREEN');
  if (!line || !line.fields[1]) return unverified('exit code and result lines disagree', 'rc=0 without SCREEN line');
  const choice = line.fields[1];
  if (choice === 'other') return { status: STATUS.UNSURE, reason: 'screen is none of the choices', choice };
  if (expect === undefined) return { status: STATUS.PASS, reason: `screen=${choice}`, choice };
  return choice === expect
    ? { status: STATUS.PASS, reason: `screen=${choice}`, choice }
    : { status: STATUS.FAIL, reason: `screen=${choice} expected=${expect}`, choice };
}

// selftest 동사: SELFTEST|PASS 한 줄과 종료코드 0.
export function toSelftestVerdict(rc, lines) {
  const parsed = parseLines(lines);
  if (rc !== 0 && rc !== 7) return nonResult(rc, parsed);
  const line = parsed.find((l) => l.tag === 'SELFTEST');
  if (rc === 0 && line?.fields[1] === 'PASS') return { status: STATUS.PASS, reason: 'selftest' };
  if (line?.fields[1] === 'FAIL') return { status: STATUS.FAIL, reason: 'selftest', detail: sanitizeDetail(line.fields.slice(2).join('|')) };
  return nonResult(rc, parsed);
}
