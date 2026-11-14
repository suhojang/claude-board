// 세션 줄 둘째 줄(정보 라벨)을 줄이는 단계 표 — mac SessionRow 의 ViewThatFits 후보 다섯과 같다(BoardViews.swift).
// Fit 이 0 단계부터 칸 폭에 다 들어가는 첫 단계를 그리고, 어느 단계도 안 들어가면 마지막 단계를 그린다.
// 마지막 단계만 라벨을 줄 폭을 넘는 것부터 다음 줄로 넘긴다(WrapRow) — 한 줄을 고집하면 세션 줄이 넓어져 카드가 보드 밖으로 밀린다.
export type MetaStage = {
  /** 하위 작업·셸·모델·컨텍스트 라벨이 줄이는 정도: 0 글자까지, 1 아이콘+개수·종류 아이콘만, 2 컨텍스트 막대·토큰도 뺌 */
  level: 0 | 1 | 2;
  /** 세션 이름 라벨을 보이는가 — 자리가 모자라면 먼저 뺀다 */
  name: boolean;
  /** 라벨을 다음 줄로 넘기는가 */
  wrap: boolean;
};

export const META_STAGES: readonly MetaStage[] = [
  { level: 0, name: true, wrap: false },
  { level: 1, name: true, wrap: false },
  { level: 1, name: false, wrap: false },
  { level: 2, name: false, wrap: false },
  { level: 2, name: false, wrap: true },
];

// Fit 은 0 에서 count-1 까지 단계 번호를 넘긴다 — 범위를 벗어나면 마지막 단계로 본다
export function metaStage(stage: number): MetaStage {
  const i = Number.isFinite(stage) ? Math.min(Math.max(Math.trunc(stage), 0), META_STAGES.length - 1) : META_STAGES.length - 1;
  return META_STAGES[i];
}
