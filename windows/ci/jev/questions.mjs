// 질문과 문턱의 유일한 자리. 사람이 검토하는 것은 이 파일의 문구와 숫자다.
// `judgment` 표시가 붙은 값은 아직 재 본 적이 없는 판단값이다(보정은 measure.mjs 절차, README 참고).

// 시각 단언 가족. 문구는 영어로, 화면의 한글은 그대로 인용한다(jev-judgment 스킬 규약).
// expect 는 이 진술이 "참이어야 정상"인 화면인지("yes") 아닌지("no")다.
// calibration.status 는 보정 전 전부 "not calibrated" — 보정 결과로만 사람이 바꾼다(자동으로 바꾸는 코드는 없다).
const notCalibrated = Object.freeze({ status: 'not calibrated', commit: null, wilsonLower: null });

export const FAMILIES = Object.freeze({
  chip: {
    statement: 'the chip text "CB" is visible in the highlighted session row',
    expect: 'yes',
  },
  'terminal-clean': {
    statement: 'no terminal lines overlap or are garbled',
    expect: 'yes',
  },
  'confirm-dialog': {
    statement: 'a confirmation dialog is shown',
    expect: 'yes',
  },
  'dialog-two-sessions': {
    statement: 'the dialog lists exactly two sessions',
    expect: 'yes',
  },
  'vertical-divider': {
    statement: 'a vertical divider separates two terminal panes',
    expect: 'yes',
  },
  'settings-screen': {
    statement: 'the settings screen with a group list on the left is shown',
    expect: 'yes',
  },
});
for (const family of Object.values(FAMILIES)) family.calibration = notCalibrated;

// screen 동사의 알려진 화면. 이름 -> 영어 설명. 모르는 화면은 하네스가 other 로 받는다.
export const SCREENS = Object.freeze({
  'board-collapsed': 'the board in its collapsed, narrow form showing only the session list',
  'board-expanded': 'the board expanded with the terminal area on the left and the session board on the right',
  settings: 'the settings screen with a group list on the left',
  'shutdown-dialog': 'a confirmation dialog asking before the board quits',
  'empty-terminal': 'the terminal area with no tab open',
});

// 보정 신뢰 문턱 (judgment, 세 값 모두). remaining-design.md §6c.
export const TRUST = Object.freeze({
  minPositives: 30, // 가족마다 양성 화면 수
  minNegatives: 30, // 가족마다 음성 화면 수
  wilsonLower: 0.9, // 결정한 답의 정확도 Wilson 하한
  falsePassMax: 1 / 30, // 정답이 아닌데 PASS 라 한 비율(음성 기준)
  abstainMax: 0.35, // UNSURE 비율
});
export const WILSON_Z = 1.96; // 95% 양쪽 구간

// 호출 상한. 하네스 기본값과 같다(jevvision.cli DEFAULT_BUDGET, judgment).
export const BUDGET = 6;
export const TIMEOUT_MS = 120_000; // judgment: 하네스 HTTP 제한 시간 60 초의 두 배

// 이미지 한도. jevvision/domain/thresholds.py 의 IMAGE_* 사본이다(출처: 공식 images.md "Limits").
// 하네스가 다시 거절하므로 이 사본이 낡아도 하네스가 이긴다. test/questions.test.mjs 가 하네스 파일이 있으면 대조한다.
export const IMAGE_LIMITS = Object.freeze({
  minSide: 280,
  maxSide: 1568,
  minPixels: 78_400,
  maxPixels: 2_458_624,
  maxFileBytes: 16 * 1024 * 1024,
});
// 디코드 전에 거절하는 원본 상한. 하네스는 줄여서 보내므로 한도 밖이어도 되지만 폭탄 이미지는 거른다 (judgment).
export const SOURCE_MAX_PIXELS = 36_000_000;
