// 종합 보고: 결정적 판정 | Jev 판정 | 일치 — 세 칸 표. Jev 는 조언자이고 결정적 증거를 바꾸지 못한다.
//
// 일치 칸 규칙(전부 결정적 판정이 기준이다):
//   둘 다 PASS 또는 둘 다 FAIL            -> agree
//   결정적 PASS 인데 Jev FAIL/UNSURE      -> DISAGREE (main 이 스크린샷을 열어 읽고 기록한다. 자동 실패 없음)
//   결정적 FAIL 인데 Jev PASS             -> DISAGREE (Jev PASS 는 결정적 FAIL 을 PASS 로 바꾸지 못한다)
//   결정적 FAIL 인데 Jev UNSURE           -> n/a
//   Jev 가 UNVERIFIED                     -> n/a (일치로 세지 않는다. 못 본 것이다)
//   결정적 판정이 PASS/FAIL 이 아님        -> n/a
import { EXIT, STATUS } from './port.mjs';

export function agreement(deterministic, jev) {
  if (jev.status === STATUS.UNVERIFIED) return 'n/a';
  if (deterministic !== STATUS.PASS && deterministic !== STATUS.FAIL) return 'n/a';
  if (jev.status === deterministic) return 'agree';
  if (deterministic === STATUS.PASS) return 'DISAGREE';
  return jev.status === STATUS.PASS ? 'DISAGREE' : 'n/a'; // 결정적 FAIL + Jev UNSURE: Jev 가 결정하지 않았으니 일치로 세지 않는다
}

function cell(verdict) {
  return verdict.reason ? `${verdict.status}: ${verdict.reason}` : verdict.status;
}

// FAIL 이 먼저, 그다음 못 본 것(UNVERIFIED·빈 표), 그다음 불확실. PASS 가 아닌 것이 하나라도 있으면 0 이 아니다.
function exitCodeOfCounts(counts, rowCount) {
  if (counts.deterministicFail > 0) return EXIT.FAIL;
  if (counts.deterministicUnverified > 0 || rowCount === 0) return EXIT.UNVERIFIED;
  if (counts.deterministicUnsure > 0) return EXIT.UNSURE;
  return EXIT.PASS;
}

// rows: [{name, deterministic: 'PASS'|'FAIL'|..., jev: Verdict}] -> {lines, exitCode, counts}
// exitCode 는 결정적 판정만으로 정한다: FAIL 이 하나라도 있으면 1, 아니면 UNVERIFIED 가 있으면 6, 아니면 UNSURE 가 있으면 3, 전부 PASS 일 때만 0.
// 행이 하나도 없으면 아무것도 확인하지 못한 것이라 6 이다. Jev 칸은 종료코드를 바꾸지 못한다.
export function aggregate(rows) {
  const table = [['check', 'deterministic', 'Jev', 'agreement']];
  const counts = { deterministicPass: 0, deterministicFail: 0, deterministicUnverified: 0, deterministicUnsure: 0, jev: { PASS: 0, FAIL: 0, UNSURE: 0, UNVERIFIED: 0 }, disagree: 0 };
  for (const row of rows) {
    if (!(row.deterministic in STATUS)) throw new TypeError(`결정적 판정이 잘못됐다: ${row.name} ${row.deterministic}`);
    const jev = row.jev ?? { status: STATUS.UNVERIFIED, reason: 'not asked' };
    const agrees = agreement(row.deterministic, jev);
    if (row.deterministic === STATUS.PASS) counts.deterministicPass += 1;
    if (row.deterministic === STATUS.FAIL) counts.deterministicFail += 1;
    if (row.deterministic === STATUS.UNVERIFIED) counts.deterministicUnverified += 1;
    if (row.deterministic === STATUS.UNSURE) counts.deterministicUnsure += 1;
    counts.jev[jev.status] += 1;
    if (agrees === 'DISAGREE') counts.disagree += 1;
    table.push([row.name, row.deterministic, cell(jev), agrees]);
  }
  const widths = table[0].map((_, i) => Math.max(...table.map((r) => r[i].length)));
  const lines = table.map((r) => r.map((c, i) => c.padEnd(widths[i])).join(' | ').trimEnd());
  lines.splice(1, 0, widths.map((w) => '-'.repeat(w)).join('-|-'));
  const j = counts.jev;
  lines.push(
    `deterministic PASS ${counts.deterministicPass} FAIL ${counts.deterministicFail} | Jev PASS ${j.PASS} FAIL ${j.FAIL} UNSURE ${j.UNSURE} UNVERIFIED ${j.UNVERIFIED} | disagreements ${counts.disagree}`,
  );
  return { lines, exitCode: exitCodeOfCounts(counts, rows.length), counts };
}
