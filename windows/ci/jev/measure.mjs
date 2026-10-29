// 보정(calibration) 측정 — remaining-design.md §6c 절차의 실행기. 키가 없으면 UNVERIFIED 로 끝난다.
//
// 절차 (사람이 하는 것과 이 파일이 하는 것):
//   1. 데이터셋을 만든다(사람/J2 청크). 보드 mock 화면(headless Chrome)과 D1 재생 렌더로 PNG 를 e2e/out/calib/ 에 모으고,
//      truth.json 에 그림마다 정답을 DOM 에서 뽑아 적는다. 음성은 DOM 변형(칩 글자 바꾸기·요소 숨기기·행 겹치기·구분선 지우기)
//      또는 다른 가족의 진술로 만든다.
//        truth.json = { "<파일>.png": { "family": "chip", "truth": "yes" | "no" } }   truth = 그 가족 진술이 그림에서 참인가
//   2. 가족마다 양성 30장 이상, 음성 30장 이상이어야 한다(TRUST.minPositives/minNegatives). 모자라면 호출 전에 UNVERIFIED 로 끝난다.
//   3. node jev.mjs measure --dataset <폴더> 가 그림마다 check 를 묻고 가족별로 센다.
//        hit      맞힌 수 / 전체(불확실도 못 맞힌 것으로 센다)
//        accuracy 맞힌 수 / 결정한 수, 그 Wilson 하한
//        abstain  UNSURE 비율
//        falsePass 정답이 아닌데 PASS 라 한 비율(음성 기준)
//   4. 신뢰 문턱(TRUST, 전부 judgment)을 넘은 가족만 사람이 questions.mjs 의 calibration.status 를 바꾼다. 이 파일은 고치지 않는다.
//   5. 결과 줄(MEASURE|...)과 가족별 표를 기록하고, 보정한 커밋을 calibration.commit 에 적는다.
//
// 어느 그림이든 UNVERIFIED 가 나오면(키 없음·예산·오류) 그 자리에서 중단한다 — 일부만 센 숫자로 신뢰를 판정하지 않는다.
// untested: 실제 호출로 끝까지 돌린 적이 없다(키가 없는 머신에서 만들었다). 순수 계산과 중단 경로만 시험했다.
import fs from 'node:fs';
import path from 'node:path';
import { FAMILIES, TRUST } from './questions.mjs';
import { STATUS, UsageError, unverified } from './port.mjs';
import { summarize, trustProblems } from './domain/stats.mjs';

export function loadDataset(directory, readFile = fs.readFileSync) {
  let truth;
  try {
    truth = JSON.parse(readFile(path.join(directory, 'truth.json'), 'utf8'));
  } catch (error) {
    throw new UsageError(`truth.json 을 읽을 수 없다: ${directory} (${error.code ?? error.message})`);
  }
  return Object.entries(truth).map(([file, entry]) => {
    if (!FAMILIES[entry?.family]) throw new UsageError(`알 수 없는 가족: ${file} -> ${entry?.family}`);
    if (entry.truth !== 'yes' && entry.truth !== 'no') throw new UsageError(`truth 는 yes 또는 no: ${file}`);
    return { file: path.join(directory, file), family: entry.family, truth: entry.truth };
  });
}

// 호출 전에 데이터셋 크기를 본다. 모자라는 가족 목록(문자열)을 돌려준다.
export function sizeProblems(items) {
  const counts = {};
  for (const { family, truth } of items) {
    counts[family] ??= { yes: 0, no: 0 };
    counts[family][truth] += 1;
  }
  return Object.entries(counts)
    .filter(([, c]) => c.yes < TRUST.minPositives || c.no < TRUST.minNegatives)
    .map(([family, c]) => `${family} pos ${c.yes}/${TRUST.minPositives} neg ${c.no}/${TRUST.minNegatives}`);
}

// -> {verdict: UNVERIFIED 판정} 또는 {families: {이름: {summary, problems}}}
export async function measure({ judge, items }) {
  if (items.length === 0) return { verdict: unverified('dataset is empty') };
  const small = sizeProblems(items);
  if (small.length > 0) return { verdict: unverified('dataset too small', small.join('; ')) };
  const byFamily = {};
  for (const item of items) {
    const verdict = await judge.check(item.file, FAMILIES[item.family].statement, FAMILIES[item.family].expect);
    if (verdict.status === STATUS.UNVERIFIED) return { verdict };
    (byFamily[item.family] ??= []).push({ truth: item.truth, status: verdict.status });
  }
  const families = {};
  for (const [name, results] of Object.entries(byFamily)) {
    const summary = summarize(results);
    families[name] = { summary, problems: trustProblems(summary) };
  }
  return { families };
}

export function measureLines(families) {
  return Object.entries(families).map(([name, { summary: s, problems }]) =>
    [
      'MEASURE',
      `family=${name}`,
      `pos=${s.positives}`,
      `neg=${s.negatives}`,
      `hit=${s.hitRate.toFixed(2)}`,
      `accuracy=${s.decidedAccuracy.toFixed(2)}`,
      `wilsonLower=${s.wilsonLower.toFixed(2)}`,
      `abstain=${s.abstainRate.toFixed(2)}`,
      `falsePass=${s.falsePassRate.toFixed(3)}`,
      problems.length === 0 ? 'trust=yes' : `trust=no(${problems.join(',')})`,
    ].join('|'),
  );
}
