// 자율주행 시작 준비 화면의 순수 부분 — 입력 칸을 검사하고, 출발 요청(run.json·queue.json 내용과 동의 표시)으로 바꾼다.
// DOM·Svelte·백엔드를 부르지 않아 node:test 로 시험한다.
//
// 규칙의 주인은 엔진 명령줄(`claude-autopilot start`)이다. 여기서는 칸이 비었는지, 숫자인지 같은 입력 모양만 보고
// 경로 규칙(절대 경로·금지 폴더)과 판정 명령의 문법은 보지 않는다. 그런 거절은 명령줄의 글이 화면에 그대로 뜬다.
// 사용자에게 보이는 글에는 긴 줄표를 쓰지 않는다.

/** 고를 수 있는 실행 환경 하나 — 백엔드 `autopilot_envs` 가 준다. 기본값은 없다 */
export interface AutopilotEnvOption {
  /** `wsl:<배포판>` · `pwsh` · `powershell` · `cmd` · `git-bash` */
  key: string;
  label: string;
  /** 종류 id: `wsl` · `pwsh` · `powershell` · `cmd` · `git-bash` */
  kind: string;
  /** 배포판 이름(WSL 만) */
  distro: string | null;
  /** Claude Code 샌드박스를 걸 수 있는 환경인가(WSL 만 참). 거짓이면 샌드박스 없음 동의가 있어야 출발한다 */
  sandboxCapable: boolean;
}

/** 단위 하나의 입력 칸 — 모두 글자다 */
export interface UnitDraft {
  title: string;
  /** 모델에게 처음 보낼 글(목표) */
  goal: string;
  /** 작업 폴더. 비우면 프로젝트 폴더 */
  cwd: string;
  /** 통과를 가르는 판정 명령(고른 실행 환경의 문법) */
  check: string;
  /** 시간 상한(분). 비우면 엔진 기본값 */
  minutes: string;
  /** 비용 상한(USD). 비우면 엔진 기본값 */
  usd: string;
}

export interface StartDraft {
  projectDir: string;
  /** 실행 이름. 비우면 프로젝트 폴더 이름 */
  title: string;
  /** 고른 실행 환경 키. 고르기 전은 null(기본값 없음) */
  envKey: string | null;
  /** 샌드박스 없음에 동의한다 */
  consent: boolean;
  model: string;
  units: UnitDraft[];
}

/** 백엔드 `autopilot_start` 로 보내는 요청 */
export interface StartRequest {
  runId: string;
  run: Record<string, unknown>;
  queue: Record<string, unknown>[];
  /** `claude-autopilot start --accept-unsandboxed` 를 붙인다 */
  acceptUnsandboxed: boolean;
}

export interface Problem {
  /** 고칠 칸: `projectDir` · `env` · `consent` · `model` · `units` · `unit.<번호>.<칸>` */
  field: string;
  message: string;
}

/** 단위 수 상한(보드가 정한 입력 상한이다. 백엔드가 같은 값을 쓴다) */
export const MAX_UNITS = 30;
/** 글 칸 길이 상한 */
export const MAX_TITLE = 120;
export const MAX_GOAL = 20000;
export const MAX_CHECK = 2000;
/** 모델 이름 꼴(`claude --model` 에 넘기는 값). macOS 출발 확인과 같은 규칙 */
export const MODEL_PATTERN = /^[a-z0-9][a-z0-9._[\]-]*$/;
/** 모델 이름 제안 — 별칭은 Claude Code 가 최신 모델로 풀어 준다 */
export const MODEL_SUGGESTIONS = ["claude-sonnet-5-5", "sonnet", "opus", "haiku"];
export const DEFAULT_MODEL = "claude-sonnet-5-5";

export function emptyUnit(): UnitDraft {
  return { title: "", goal: "", cwd: "", check: "", minutes: "", usd: "" };
}

export function emptyDraft(): StartDraft {
  return { projectDir: "", title: "", envKey: null, consent: false, model: DEFAULT_MODEL, units: [emptyUnit()] };
}

/** 경로의 마지막 이름(`\`·`/` 모두 구분자, 끝 구분자 무시). 없으면 빈 글 */
export function folderName(path: string): string {
  const parts = path.trim().replace(/[\\/]+$/, "").split(/[\\/]/);
  return parts.at(-1)?.trim() ?? "";
}

/** 실행 id 에 쓸 이름 조각 — 영문·숫자·점·밑줄·붙임표만 남기고 나머지는 붙임표 하나로. 비면 `run` */
export function slug(name: string): string {
  const cleaned = name
    .replace(/[^A-Za-z0-9._-]+/g, "-")
    .replace(/-{2,}/g, "-")
    .replace(/^[-.]+|[-.]+$/g, "")
    .slice(0, 40)
    .replace(/[-.]+$/g, "");
  return cleaned || "run";
}

const two = (n: number) => String(n).padStart(2, "0");

/** 실행 id: `YYYYMMDD-HHMMSS-<프로젝트 폴더 이름 조각>`(지역 시각). 실행 id 규칙(영문·숫자·점·밑줄·붙임표, 64자 이하)에 맞다 */
export function makeRunId(now: Date, projectDir: string): string {
  const stamp = `${now.getFullYear()}${two(now.getMonth() + 1)}${two(now.getDate())}-${two(now.getHours())}${two(now.getMinutes())}${two(now.getSeconds())}`;
  return `${stamp}-${slug(folderName(projectDir))}`;
}

/** 상한 칸 읽기: 빈 칸은 `null`(엔진 기본값), 0보다 큰 유한한 수는 그 수, 그 밖은 `"bad"` */
export function parseCap(text: string): number | null | "bad" {
  const t = text.trim();
  if (t === "") return null;
  if (!/^\d+(\.\d+)?$/.test(t)) return "bad";
  const n = Number(t);
  return Number.isFinite(n) && n > 0 ? n : "bad";
}

/** 고른 환경 */
export function envOf(draft: StartDraft, envs: AutopilotEnvOption[]): AutopilotEnvOption | null {
  return envs.find((e) => e.key === draft.envKey) ?? null;
}

/** 이 환경은 샌드박스 없음에 동의해야 출발한다 */
export function needsConsent(env: AutopilotEnvOption | null): boolean {
  return env !== null && !env.sandboxCapable;
}

/** 판정 명령과 작업 폴더를 어느 문법으로 쓰는가 — 고른 환경에 따라 화면이 칸 아래에 보이는 안내 */
export function syntaxHint(env: AutopilotEnvOption | null): string {
  if (env === null) return "실행 환경을 고르면 그 환경의 문법으로 씁니다.";
  switch (env.kind) {
    case "wsl":
      return `bash 문법으로 씁니다. 작업 폴더는 \\\\wsl.localhost\\${env.distro ?? "배포판"}\\ 아래 경로여야 합니다.`;
    case "git-bash":
      return "bash 문법으로 씁니다(Git Bash).";
    case "cmd":
      return "명령 프롬프트 문법으로 씁니다.";
    default:
      return "PowerShell 문법으로 씁니다.";
  }
}

/** 칸 검사. 문제가 없으면 빈 목록 */
export function validateDraft(draft: StartDraft, envs: AutopilotEnvOption[]): Problem[] {
  const out: Problem[] = [];
  const add = (field: string, message: string) => out.push({ field, message });

  if (draft.projectDir.trim() === "") add("projectDir", "프로젝트 폴더를 고르세요.");
  if (draft.title.trim().length > MAX_TITLE) add("title", `실행 이름은 ${MAX_TITLE}자 이하로 쓰세요.`);

  const env = envOf(draft, envs);
  if (draft.envKey === null) add("env", "실행 환경을 고르세요. 기본값은 없습니다.");
  else if (env === null) add("env", "고른 실행 환경이 이 PC 목록에 없습니다. 다시 고르세요.");
  else if (needsConsent(env) && !draft.consent) add("consent", "이 환경은 샌드박스 없이 돕니다. 동의하면 출발할 수 있습니다.");

  const model = draft.model.trim();
  if (model === "") add("model", "모델을 쓰세요.");
  else if (!MODEL_PATTERN.test(model)) add("model", "모델 이름은 영문 소문자·숫자와 . - _ [ ] 만 쓸 수 있습니다.");

  if (draft.units.length === 0) add("units", "단위를 하나 이상 추가하세요.");
  if (draft.units.length > MAX_UNITS) add("units", `단위는 ${MAX_UNITS}개까지 담을 수 있습니다.`);
  draft.units.forEach((u, i) => {
    const at = (field: string) => `unit.${i + 1}.${field}`;
    const n = i + 1;
    if (u.title.trim() === "") add(at("title"), `단위 ${n}: 제목을 쓰세요.`);
    else if (u.title.trim().length > MAX_TITLE) add(at("title"), `단위 ${n}: 제목은 ${MAX_TITLE}자 이하로 쓰세요.`);
    if (u.goal.trim() === "") add(at("goal"), `단위 ${n}: 목표(모델에게 처음 보낼 글)를 쓰세요.`);
    else if (u.goal.trim().length > MAX_GOAL) add(at("goal"), `단위 ${n}: 목표는 ${MAX_GOAL}자 이하로 쓰세요.`);
    if (u.check.trim() === "") add(at("check"), `단위 ${n}: 판정 명령을 쓰세요. 통과 여부를 이 명령의 종료 코드로 가릅니다.`);
    else if (u.check.trim().length > MAX_CHECK) add(at("check"), `단위 ${n}: 판정 명령은 ${MAX_CHECK}자 이하로 쓰세요.`);
    if (parseCap(u.minutes) === "bad") add(at("minutes"), `단위 ${n}: 시간 상한은 0보다 큰 숫자(분)로 쓰세요.`);
    if (parseCap(u.usd) === "bad") add(at("usd"), `단위 ${n}: 비용 상한은 0보다 큰 숫자(USD)로 쓰세요.`);
  });
  return out;
}

/** 단위 하나 → `queue.json` 의 한 칸. 키는 이름 순(macOS 가 쓴 파일과 같은 순서) */
export function buildUnit(u: UnitDraft, n: number, projectDir: string): Record<string, unknown> {
  const unit: Record<string, unknown> = {};
  const minutes = parseCap(u.minutes);
  const usd = parseCap(u.usd);
  if (typeof minutes === "number" || typeof usd === "number") {
    const caps: Record<string, number> = {};
    if (typeof minutes === "number") caps.minutes = minutes;
    if (typeof usd === "number") caps.usd = usd;
    unit.caps = caps;
  }
  unit.check = u.check.trim();
  unit.cwd = u.cwd.trim() || projectDir.trim();
  unit.id = `u${n}`;
  unit.prompt = u.goal.trim();
  unit.title = u.title.trim();
  return unit;
}

/** 고른 환경 → `run.json` 의 `exec_env`. 동의 기록(`unsandboxed_ok`)은 쓰지 않는다: 명령줄이 `--accept-unsandboxed` 로 남긴다 */
export function buildExecEnv(env: AutopilotEnvOption): Record<string, unknown> {
  return env.distro === null ? { key: env.key, kind: env.kind } : { distro: env.distro, key: env.key, kind: env.kind };
}

export type BuildResult = { ok: true; request: StartRequest } | { ok: false; problems: Problem[] };

/** 칸 → 출발 요청. 검사에 걸리면 요청을 만들지 않는다 */
export function buildStart(draft: StartDraft, envs: AutopilotEnvOption[], now: Date): BuildResult {
  const problems = validateDraft(draft, envs);
  const env = envOf(draft, envs);
  if (problems.length > 0 || env === null) return { ok: false, problems };

  const projectDir = draft.projectDir.trim();
  const name = folderName(projectDir);
  const run: Record<string, unknown> = {
    exec_env: buildExecEnv(env),
    model: draft.model.trim(),
    project: name,
    project_root: projectDir,
    title: draft.title.trim() || name,
  };
  return {
    ok: true,
    request: {
      runId: makeRunId(now, projectDir),
      run,
      queue: draft.units.map((u, i) => buildUnit(u, i + 1, projectDir)),
      acceptUnsandboxed: needsConsent(env) && draft.consent,
    },
  };
}

/** 문제 목록에서 한 칸의 글 */
export function problemFor(problems: Problem[], field: string): string | null {
  return problems.find((p) => p.field === field)?.message ?? null;
}
