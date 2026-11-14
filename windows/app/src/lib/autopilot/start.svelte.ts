// 자율주행 시작 준비 화면의 상태와 동작 — 열고 닫기, 단위 더하기·빼기, 출발.
// 칸 검사와 출발 요청 만들기는 startForm.ts(순수, 시험 있음). 여기는 그 결과대로 상태를 바꾸고 백엔드를 부른다.
// 닫아도 쓰던 칸은 남는다(실수로 닫아도 다시 쓰지 않게). 출발에 성공하면 비운다.
import { autopilotEnvs, autopilotStart } from "$lib/backend";
import { buildStart, emptyDraft, emptyUnit, MAX_UNITS, type AutopilotEnvOption, type StartDraft } from "./startForm";

export const AutopilotStart = $state({
  /** 화면이 떠 있다 */
  shown: false,
  draft: emptyDraft() as StartDraft,
  envs: [] as AutopilotEnvOption[],
  /** 실행 환경 목록을 한 번이라도 받았다 */
  envsLoaded: false,
  /** 시작을 눌러 본 적이 있다 — 그 뒤에야 칸마다 문제 글을 보인다 */
  tried: false,
  starting: false,
  /** 명령줄(또는 백엔드)이 거절한 글. 여러 줄일 수 있다 */
  error: "",
});

async function loadEnvs() {
  try {
    AutopilotStart.envs = await autopilotEnvs();
  } catch (e) {
    console.warn("자율주행 실행 환경 목록을 받지 못함", e);
    AutopilotStart.envs = [];
  } finally {
    AutopilotStart.envsLoaded = true;
  }
}

/** 화면을 연다. 실행 환경 목록은 열 때마다 다시 받는다(WSL 을 새로 깔았을 수 있다) */
export function openAutopilotStart(): void {
  AutopilotStart.error = "";
  AutopilotStart.shown = true;
  void loadEnvs();
}

export function closeAutopilotStart(): void {
  AutopilotStart.shown = false;
}

export function addUnit(): void {
  if (AutopilotStart.draft.units.length < MAX_UNITS) AutopilotStart.draft.units.push(emptyUnit());
}

export function removeUnit(index: number): void {
  AutopilotStart.draft.units.splice(index, 1);
}

/** 고른 환경이 바뀌면 동의 표시를 내린다 — 다른 환경에 대한 동의가 따라가지 않게 */
export function chooseEnv(key: string): void {
  if (AutopilotStart.draft.envKey === key) return;
  AutopilotStart.draft.envKey = key;
  AutopilotStart.draft.consent = false;
}

/** 출발. 칸 검사에 걸리면 백엔드를 부르지 않는다. 명령줄이 거절하면 그 글을 화면에 남기고 열어 둔다 */
export async function submitStart(): Promise<void> {
  if (AutopilotStart.starting) return;
  AutopilotStart.tried = true;
  AutopilotStart.error = "";
  const built = buildStart(AutopilotStart.draft, AutopilotStart.envs, new Date());
  if (!built.ok) return;
  AutopilotStart.starting = true;
  try {
    await autopilotStart(built.request);
    AutopilotStart.draft = emptyDraft();
    AutopilotStart.tried = false;
    closeAutopilotStart();
  } catch (e) {
    AutopilotStart.error = e instanceof Error ? e.message : String(e);
  } finally {
    AutopilotStart.starting = false;
  }
}
