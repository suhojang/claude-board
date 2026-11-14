// 자율주행 시작 확인의 "승격 차례" 줄 — 순수 부분. 선택한 프로젝트의 승격 차례(`projects/<프로젝트>.json` promotion)를 줄 글로 바꾸고,
// 바꾸기 화면이 쓰는 규칙(고칠 것·고를 것·채우기·바꾸기)을 옮긴다. DOM·Svelte·백엔드를 부르지 않는다.
// 원본: Sources/ClaudeBoard/Autopilot.swift(`promotionRow`·`promotionEditor`·`pickDefaultBases`·`promotionBranches`)와
// Sources/AutopilotRules/Promotion.swift(`apBranchRoles`·`apDefaultPromotion`·`apPromotionProblems`·`apPromotionChoices`).
// Rust 판: windows/crates/autopilot/src/rules/promotion.rs. 가지 이름·경로 글은 글 그대로 견주고 보여 준다(대소문자·`\`·드라이브 글자를 고치지 않는다).
// 사용자에게 보이는 글에는 긴 줄표를 쓰지 않는다.

/** 이 실행의 자율주행 가지를 가리키는 `from` 값 */
export const FROM_AUTOPILOT = "autopilot";

/** 승격 차례 한 단계 — `from` 이 `autopilot` 이면 자율주행 가지. `after` 가 없으면 null */
export interface PromotionStep {
  from: string;
  into: string;
  after: string | null;
}

export interface BranchRoles {
  dev: string | null;
  live: string | null;
  master: string | null;
}

export interface PromotionChoices {
  /** 받는 가지 */
  into: string[];
  /** 올릴 것 */
  from: string[];
  /** 뒤 조건 */
  after: string[];
}

const ROW_MARKERS = ["①", "②", "③", "④", "⑤"];
const CIRCLED = "①②③④⑤⑥⑦⑧⑨⑩";

/** 단계 번호 글자(1부터, 10 까지 동그라미 숫자, 그 밖은 `n.`). 원본: `apCircled` */
export function circled(n: number): string {
  return n >= 1 && n <= 10 ? Array.from(CIRCLED)[n - 1] : `${n}.`;
}

/** 줄 안 단계 번호 글자 — 다섯째까지 동그라미 숫자, 그다음은 `n.`. 원본: `promotionRow` 의 nums */
export function rowMarker(index: number): string {
  return index < ROW_MARKERS.length ? ROW_MARKERS[index] : `${index + 1}.`;
}

/** 올릴 것 글 — 자율주행 가지는 Jira 연결이면 "스토리 가지", 아니면 "autopilot 가지", 그 밖은 가지 이름 그대로 */
export function fromLabel(from: string, jira: boolean): string {
  return from === FROM_AUTOPILOT ? (jira ? "스토리 가지" : "autopilot 가지") : from;
}

/** 단계 하나의 글 — `live ← autopilot 가지 (dev 뒤)` */
export function stepSentence(step: PromotionStep, jira: boolean): string {
  return `${step.into} ← ${fromLabel(step.from, jira)}` + (step.after === null ? "" : ` (${step.after} 뒤)`);
}

const firstOf = (names: string[], b: string[]): string | null => names.find((n) => b.includes(n)) ?? null;

/** 가지 자리: dev(develop), live(staging), master(main). 앞 이름이 먼저다. 원본: `apBranchRoles` */
export function branchRoles(b: string[]): BranchRoles {
  return { dev: firstOf(["dev", "develop"], b), live: firstOf(["live", "staging"], b), master: firstOf(["master", "main"], b) };
}

/** 프로젝트 설정에 승격 차례가 없을 때 저장소 가지로 채운 차례. 원본: `apDefaultPromotion` */
export function defaultPromotion(branches: string[]): PromotionStep[] {
  const n = branchRoles(branches);
  const st: PromotionStep[] = [];
  if (n.dev !== null) st.push({ from: FROM_AUTOPILOT, into: n.dev, after: null });
  if (n.live !== null) st.push({ from: FROM_AUTOPILOT, into: n.live, after: n.dev });
  if (n.master !== null) {
    st.push(n.live !== null ? { from: n.live, into: n.master, after: n.live } : { from: FROM_AUTOPILOT, into: n.master, after: n.dev });
  }
  return st;
}

/**
 * 프로젝트 설정 `promotion` 값(읽은 JSON 그대로)에서 승격 차례를 꺼낸다. 비어 있지 않은 목록이면 설정의 차례(`saved` 참),
 * 그렇지 않으면 첫 저장소의 가지로 채운다(`saved` 거짓). `from`·`into` 가 글이 아닌 단계는 건너뛴다. 원본: `pickDefaultBases` 끝
 */
export function loadPromotion(raw: unknown, repoBranches: string[][]): { steps: PromotionStep[]; saved: boolean } {
  if (Array.isArray(raw) && raw.length > 0) {
    const steps: PromotionStep[] = [];
    for (const s of raw) {
      if (s === null || typeof s !== "object") continue;
      const o = s as Record<string, unknown>;
      if (typeof o.from !== "string" || typeof o.into !== "string") continue;
      steps.push({ from: o.from, into: o.into, after: typeof o.after === "string" ? o.after : null });
    }
    return { steps, saved: true };
  }
  return { steps: defaultPromotion(repoBranches[0] ?? []), saved: false };
}

/** 차례에 쓸 수 있는 가지 — 묶음이면 모든 저장소에 있는 가지만, `autopilot/` 로 시작하는 가지는 뺀다. 원본: `promotionBranches` */
export function promotionBranches(repoBranches: string[][]): string[] {
  if (repoBranches.length === 0) return [];
  let common = repoBranches[0];
  for (const l of repoBranches.slice(1)) common = common.filter((b) => l.includes(b));
  return common.filter((b) => !b.startsWith("autopilot/"));
}

/** 바꾼 승격 차례의 고칠 것. 원본: `apPromotionProblems` */
export function promotionProblems(steps: PromotionStep[], branches: string[]): string[] {
  const out: string[] = [];
  const seen: string[] = [];
  const intos: string[] = [];
  steps.forEach((s, i) => {
    const n = `단계 ${i + 1}`;
    if (!branches.includes(s.into)) out.push(`${n}: 받는 가지 ${s.into} 가 저장소에 없습니다`);
    if (s.from !== FROM_AUTOPILOT) {
      if (s.from === s.into) out.push(`${n}: 같은 가지(${s.into})끼리는 올리지 않습니다`);
      else if (!branches.includes(s.from)) out.push(`${n}: 올릴 가지 ${s.from} 가 저장소에 없습니다`);
      else if (!intos.includes(s.from)) out.push(`${n}: ${s.from} 는 앞 단계가 받는 가지가 아닙니다(자율주행 가지를 먼저 그 가지에 합치는 단계를 앞에 둡니다)`);
    }
    if (s.after !== null) {
      if (s.after === s.into) out.push(`${n}: 뒤 조건이 받는 가지 자신(${s.after})입니다`);
      else if (!intos.includes(s.after)) out.push(`${n}: ${s.after} 뒤라는 조건의 단계가 앞에 없습니다`);
    }
    const key = `${s.from}>${s.into}`;
    if (seen.includes(key)) out.push(`${n}: 같은 단계(${s.from === FROM_AUTOPILOT ? "자율주행 가지" : s.from} → ${s.into})가 두 번 있습니다`);
    seen.push(key);
    if (!intos.includes(s.into)) intos.push(s.into);
  });
  return out;
}

/** 바꾸기 화면의 고를 수 있는 것. `at` 이 단계 수 이상이면 새 단계다. 원본: `apPromotionChoices` */
export function promotionChoices(steps: PromotionStep[], at: number, branches: string[]): PromotionChoices {
  const prev: string[] = [];
  for (const s of steps.slice(0, Math.max(0, Math.min(at, steps.length)))) if (!prev.includes(s.into)) prev.push(s.into);
  const from = at >= 0 && at < steps.length ? steps[at].from : FROM_AUTOPILOT;
  return { into: branches.filter((b) => b !== from), from: [FROM_AUTOPILOT, ...prev], after: prev };
}

// 바꾸기 동작: 새 배열을 돌려준다(입력은 고치지 않는다). 원본: `setPromotion`·`addPromotionStep`·`removePromotionStep`·`refillPromotion`

/** 단계 `i` 의 칸을 바꾼다. 주지 않은 칸(undefined)은 그대로, `after` 는 null 이 "없음"이다. 범위 밖 `i` 는 바꾸지 않는다 */
export function setStep(steps: PromotionStep[], i: number, patch: { from?: string; into?: string; after?: string | null }): PromotionStep[] {
  if (!Number.isInteger(i) || i < 0 || i >= steps.length) return steps.map((s) => ({ ...s }));
  return steps.map((s, k) =>
    k === i ? { from: patch.from ?? s.from, into: patch.into ?? s.into, after: patch.after === undefined ? s.after : patch.after } : { ...s },
  );
}

/** 단계를 하나 더한다 — 받는 가지는 아직 안 쓴 첫 가지, 없으면 첫 가지, 가지가 없으면 `dev`. 올릴 것은 앞 단계가 받는 가지(없으면 자율주행 가지) */
export function addStep(steps: PromotionStep[], branches: string[]): PromotionStep[] {
  const used = new Set(steps.map((s) => s.into));
  const into = branches.find((b) => !used.has(b)) ?? branches[0] ?? "dev";
  const last = steps.length > 0 ? steps[steps.length - 1].into : null;
  return [...steps.map((s) => ({ ...s })), { from: last ?? FROM_AUTOPILOT, into, after: last }];
}

export function removeStep(steps: PromotionStep[], i: number): PromotionStep[] {
  return steps.filter((_, k) => k !== i).map((s) => ({ ...s }));
}

/** 저장소 가지로 다시 채운다 */
export function refillSteps(branches: string[]): PromotionStep[] {
  return defaultPromotion(branches);
}

// 줄

export const PROMOTION_LABEL = "승격 차례";
export const PROMOTION_EMPTY = "승격할 가지(dev·live·master)가 없습니다";
export const PROMOTION_JIRA_PREFIX = "스토리마다 ";
export const NOTE_EDITED = "바꾼 차례입니다. 출발하면 프로젝트 설정에 덮어씁니다(같은 프로젝트의 끝난 실행 합치기·승격도 이 차례를 따릅니다).";
export const NOTE_SAVED = "프로젝트 설정의 차례입니다.";
export const NOTE_FILLED = "저장소 가지로 채웠습니다. 출발하면 프로젝트 설정에 남깁니다.";
export const PROBLEM_PREFIX = "· ";
export const EDIT_OPEN = "프로젝트 설정 바꾸기";
export const EDIT_CLOSE = "바꾸기 닫기";
export const HELP_NO_REPOS = "저장소 가지를 읽은 뒤에 바꿀 수 있습니다";
export const HELP_EDIT = "승격 차례를 이 자리에서 바꿉니다";
export const UNDO_TITLE = "되돌리기";
export const UNDO_HELP = "바꾸기 전 차례로";
export const BRANCHES_NONE = "없음";
export const BRANCHES_LABEL = "받을 수 있는 가지: ";
export const BRANCHES_BUNDLE = "(묶음의 모든 저장소에 있는 가지)";

/** 줄이 받는 값 — 모두 글자·숫자·참거짓·배열이다 */
export interface PromotionLineInput {
  /** 지금 차례(설정에서 읽었거나 채웠거나 바꾼 것) */
  steps: PromotionStep[];
  /** 저장소마다 가지 이름 목록(묶음이면 저장소 수만큼). 저장소를 아직 못 읽었으면 빈 배열 */
  repoBranches: string[][];
  /** Jira 연결 실행이면 참 — 자율주행 가지가 "스토리 가지"로 보이고 줄 앞에 "스토리마다 "가 붙는다 */
  jira: boolean;
  /** 이 자리에서 바꿨는가 */
  edited: boolean;
  /** 차례가 프로젝트 설정에 있던 것인가(거짓이면 저장소 가지로 채운 것) */
  saved: boolean;
  /** 출발 중인가 */
  starting: boolean;
  /** 바꾸기 화면이 열려 있는가(단추 글만 바뀐다) */
  editorOpen: boolean;
}

export type NoteTone = "neutral" | "limit";

export interface PromotionSegment {
  /** 단계 번호 글자 */
  marker: string;
  into: string;
  /** 보이는 올릴 것 글(자율주행 가지는 풀어 쓴 글) */
  from: string;
  after: string | null;
  /** `① live ← dev (dev 뒤)` */
  text: string;
}

export interface PromotionLine {
  label: string;
  /** 줄 본문 — Jira 연결이면 "스토리마다 " 가 앞에 붙고, 차례가 비면 PROMOTION_EMPTY */
  text: string;
  /** 단계가 하나도 없는가 */
  empty: boolean;
  segments: PromotionSegment[];
  /** 본문 아래 한 줄 안내와 그 색 */
  note: string;
  noteTone: NoteTone;
  /** 고칠 것(앞에 `· ` 가 붙지 않은 글). 바꾼 차례든 아니든 계산한다 */
  problems: string[];
  /** 고칠 것 줄의 색 — 바꾼 차례는 "fail"(출발을 잠근다), 아니면 "limit"(알림) */
  problemTone: "fail" | "limit";
  /** 바꾼 차례에 고칠 것이 있어 출발을 잠그는가 */
  locksStart: boolean;
  /** 출발 막힘 목록에 들어갈 까닭(없으면 null) */
  startBlock: string | null;
  /** 「프로젝트 설정 바꾸기」 단추를 누를 수 있는가 — 출발 중이 아니고 저장소 가지를 읽은 뒤 */
  editable: boolean;
  editButtonTitle: string;
  editButtonHelp: string;
  /** 「되돌리기」 단추가 보이는가 */
  canUndo: boolean;
  /** 「＋ 단계 더하기」·「저장소 가지로 다시 채우기」 를 누를 수 있는가(쓸 가지가 있을 때) */
  canAdd: boolean;
  /** 차례에 쓸 수 있는 가지 */
  branches: string[];
  /** 바꾸기 화면 아래 "받을 수 있는 가지: ..." 글 */
  branchesNote: string;
}

/** 승격 차례 줄. 원본: `promotionRow`(줄·안내·고칠 것·단추)와 `promotionEditor`(가지 안내) */
export function promotionLine(input: PromotionLineInput): PromotionLine {
  const { steps, repoBranches, jira, edited, saved, starting, editorOpen } = input;
  const branches = promotionBranches(repoBranches);
  const segments: PromotionSegment[] = steps.map((st, i) => {
    const marker = rowMarker(i);
    return { marker, into: st.into, from: fromLabel(st.from, jira), after: st.after, text: `${marker} ${stepSentence(st, jira)}` };
  });
  const joined = segments.map((s) => s.text).join(" · ");
  const problems = promotionProblems(steps, branches);
  const locksStart = edited && problems.length > 0;
  const hasRepos = repoBranches.length > 0;
  return {
    label: PROMOTION_LABEL,
    text: (jira ? PROMOTION_JIRA_PREFIX : "") + (joined === "" ? PROMOTION_EMPTY : joined),
    empty: steps.length === 0,
    segments,
    note: edited ? NOTE_EDITED : saved ? NOTE_SAVED : NOTE_FILLED,
    noteTone: edited ? "limit" : "neutral",
    problems,
    problemTone: edited ? "fail" : "limit",
    locksStart,
    startBlock: locksStart ? `바꾼 승격 차례에 고칠 것이 ${problems.length}개 있습니다` : null,
    editable: !starting && hasRepos,
    editButtonTitle: editorOpen ? EDIT_CLOSE : EDIT_OPEN,
    editButtonHelp: hasRepos ? HELP_EDIT : HELP_NO_REPOS,
    canUndo: edited,
    canAdd: branches.length > 0,
    branches,
    branchesNote: BRANCHES_LABEL + (branches.length === 0 ? BRANCHES_NONE : branches.join(" · ")) + (repoBranches.length > 1 ? BRANCHES_BUNDLE : ""),
  };
}
