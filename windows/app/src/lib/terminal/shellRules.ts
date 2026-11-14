// 터미널 셸 고르기 규칙 — 백엔드 terminal.rs(can_type_claude·EnvChoice 키)와 같은 약속을 순수 함수로 쥔다. node:test 로 바로 시험한다.
// 셸 키: `pwsh` · `powershell` · `cmd` · `git-bash` · `wsl` · `wsl:<배포판>`
import type { EnvChoice } from "../types.ts";

const FIXED_KEYS = new Set(["pwsh", "powershell", "cmd", "git-bash"]);
// 배포판 이름에 쓰는 글자 — 백엔드가 "wsl:bad name"·"wsl:a;b" 를 거절하는 것과 같다
const WSL_KEY = /^wsl(:[A-Za-z0-9._-]+)?$/;

/** 이 셸 탭에 보드가 claude 명령을 쳐 넣어도 되는가 — WSL 은 세션 파일의 tty·pid 로 탭을 짝지을 수 없어 쳐 넣지 않는다 */
export function canTypeClaude(shellKey: string): boolean {
  return !shellKey.startsWith("wsl");
}

/** 셸 키를 사람이 읽는 이름으로 — 목록에 없는 키는 키 그대로 보인다(이 PC 에서 사라진 배포판 등) */
export function labelFor(shellKey: string, envs: readonly EnvChoice[]): string {
  return envs.find((env) => env.key === shellKey)?.label ?? shellKey;
}

/** 디스크·설정에서 읽은 값을 셸 키로 — 모양이 맞지 않으면 null(기본 셸). 이 PC 에 있는지는 보지 않는다 */
export function normalizeShellKey(raw: unknown): string | null {
  if (typeof raw !== "string") return null;
  const key = raw.trim();
  return FIXED_KEYS.has(key) || WSL_KEY.test(key) ? key : null;
}

/** 셸 고르기 목록의 한 줄 — key 가 null 이면 "자동(기본)", 곧 설정의 기본 셸 순서(pwsh → powershell → cmd)를 따른다 */
export interface ShellChoice {
  key: string | null;
  label: string;
}

/** 설정 화면·+ 메뉴가 같이 쓰는 목록 — "자동(기본)" 이 맨 앞, 뒤는 이 PC 의 실행 환경 순서 그대로 */
export function shellChoices(envs: readonly EnvChoice[]): ShellChoice[] {
  return [{ key: null, label: "자동(기본)" }, ...envs.map((env) => ({ key: env.key, label: env.label }))];
}
