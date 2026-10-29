// JudgePort — Jev 를 부르는 쪽이 아는 유일한 모양. HTTP 는 하네스 jevvision 이 소유하고 여기에는 없다.
//
//   interface Judge {
//     check(imagePath, statement, expect = 'yes') -> Promise<Verdict>            // statement: 문자열 또는 {name, statement, expect}
//     checkAll(imagePath, items: [{name, statement, expect}]) -> Promise<Verdict[]>   // 같은 화면의 질문을 한 요청에 묶는다
//     screen(imagePath, choices: string[], expect?) -> Promise<Verdict>          // verdict.choice = 고른 화면
//   }
//   Verdict = { status: 'PASS'|'FAIL'|'UNSURE'|'UNVERIFIED', reason: string, noul?: number, choice?: string, detail?: string }
//
// 비-PASS 가 PASS 로 바뀌는 길은 어디에도 없다. UNVERIFIED 는 "못 봤다"이지 통과가 아니다.

// 호출자의 잘못된 입력(질문·인자). CLI 는 USAGE 로 보고하고 종료코드 2 를 낸다. 판정이 아니다.
export class UsageError extends Error {}

export const STATUS = Object.freeze({ PASS: 'PASS', FAIL: 'FAIL', UNSURE: 'UNSURE', UNVERIFIED: 'UNVERIFIED' });

// 종료코드. 하네스 jevvision.cli 와 맞춘다: 0 통과, 1 실패, 3 불확실, 6 확인 못 함.
export const EXIT = Object.freeze({ PASS: 0, FAIL: 1, USAGE: 2, UNSURE: 3, GUARD: 5, UNVERIFIED: 6 });

export function exitCodeOf(status) {
  if (!(status in EXIT) || status === 'USAGE' || status === 'GUARD') throw new Error(`판정이 아닌 상태: ${status}`);
  return EXIT[status];
}

export function unverified(reason, detail) {
  return detail ? { status: STATUS.UNVERIFIED, reason, detail } : { status: STATUS.UNVERIFIED, reason };
}

// check 의 두 번째 인자를 {name, statement, expect} 로 정규화한다.
export function normalizeItem(item, expect = 'yes', index = 0) {
  const base = typeof item === 'string' ? { statement: item } : { ...item };
  if (typeof base.statement !== 'string' || !base.statement.trim()) throw new UsageError('statement 가 비었다');
  base.expect = base.expect ?? expect;
  if (base.expect !== 'yes' && base.expect !== 'no') throw new UsageError(`expect 는 yes 또는 no: ${base.expect}`);
  base.name = base.name ?? `q${index + 1}`;
  if (!/^[A-Za-z0-9_-]+$/.test(base.name)) throw new UsageError(`이름은 영문·숫자·_-: ${base.name}`);
  return base;
}

// 어댑터가 포트를 지켰는지 확인하는 모양 검사.
export function assertJudge(judge) {
  for (const method of ['check', 'checkAll', 'screen']) {
    if (typeof judge?.[method] !== 'function') throw new TypeError(`Judge.${method} 가 없다`);
  }
  return judge;
}

export function assertVerdict(verdict) {
  if (!verdict || !(verdict.status in STATUS)) throw new TypeError(`판정 상태가 잘못됐다: ${verdict?.status}`);
  if (typeof verdict.reason !== 'string') throw new TypeError('판정에 reason 이 없다');
  return verdict;
}
