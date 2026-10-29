// 보정 통계 — 순수 함수. 문턱은 questions.mjs 의 TRUST 가 소유한다.
import { TRUST, WILSON_Z } from '../questions.mjs';

export function wilsonLower(correct, total, z = WILSON_Z) {
  if (total === 0) return 0;
  const p = correct / total;
  const z2 = z * z;
  const centre = p + z2 / (2 * total);
  const margin = z * Math.sqrt((p * (1 - p)) / total + z2 / (4 * total * total));
  return Math.max(0, (centre - margin) / (1 + z2 / total));
}

// results: [{truth: 'yes'|'no', status}] — 진술이 expect=yes 일 때 PASS = "예"라 했다, FAIL = "아니오"라 했다.
// UNVERIFIED 는 여기 들어오지 않는다(호출자가 중단한다). 들어오면 오류다 — 못 본 것을 맞힌 것으로도 틀린 것으로도 세지 않는다.
export function summarize(results) {
  let positives = 0;
  let negatives = 0;
  let decided = 0;
  let correct = 0;
  let abstain = 0;
  let falsePass = 0;
  for (const { truth, status } of results) {
    if (truth !== 'yes' && truth !== 'no') throw new TypeError(`truth 는 yes 또는 no: ${truth}`);
    if (status !== 'PASS' && status !== 'FAIL' && status !== 'UNSURE') throw new TypeError(`센 수 없는 상태: ${status}`);
    if (truth === 'yes') positives += 1;
    else negatives += 1;
    if (status === 'UNSURE') {
      abstain += 1;
      continue;
    }
    decided += 1;
    if ((status === 'PASS') === (truth === 'yes')) correct += 1;
    else if (status === 'PASS') falsePass += 1;
  }
  const total = positives + negatives;
  return {
    total,
    positives,
    negatives,
    decided,
    correct,
    hitRate: total ? correct / total : 0, // 불확실도 못 맞힌 것으로 센다
    decidedAccuracy: decided ? correct / decided : 0,
    wilsonLower: wilsonLower(correct, decided),
    abstainRate: total ? abstain / total : 0,
    falsePass,
    falsePassRate: negatives ? falsePass / negatives : 0,
  };
}

// TRUST 문턱 판정. 사유 목록이 비어야 신뢰한다.
export function trustProblems(summary) {
  const problems = [];
  if (summary.positives < TRUST.minPositives) problems.push(`positives ${summary.positives}<${TRUST.minPositives}`);
  if (summary.negatives < TRUST.minNegatives) problems.push(`negatives ${summary.negatives}<${TRUST.minNegatives}`);
  if (summary.wilsonLower < TRUST.wilsonLower) problems.push(`wilsonLower ${summary.wilsonLower.toFixed(2)}<${TRUST.wilsonLower}`);
  if (summary.falsePassRate > TRUST.falsePassMax) problems.push(`falsePassRate ${summary.falsePassRate.toFixed(3)}>${TRUST.falsePassMax.toFixed(3)}`);
  if (summary.abstainRate > TRUST.abstainMax) problems.push(`abstainRate ${summary.abstainRate.toFixed(2)}>${TRUST.abstainMax}`);
  return problems;
}
