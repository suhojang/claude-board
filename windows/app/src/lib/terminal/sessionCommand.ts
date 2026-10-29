// 셸에 치는 claude 명령줄을 만드는 순수 함수 — launch.ts 에서 떼어 냈다. sessionName.ts 말고는 import 하지 않아 node:test 로 바로 시험한다.
// macOS 보드의 ClaudeBoard.swift freshSessionCommand 와 ClaudeBoard.swift TerminalHub.named에 맞춘 것이다.
import { isSessionNameArg } from "./sessionName.ts";

/** 새 세션 기본 명령 — 승인 대기가 거의 없게 권한 확인을 건너뛴다(ClaudeBoard.swift defaultSessionCommand) */
export const DEFAULT_SESSION_COMMAND = "claude --dangerously-skip-permissions";

// claude 로 시작하는 명령 — 앞의 경로(\ 또는 /)와 .exe 는 있어도 된다
export const CLAUDE_START = /^\s*(\S*[\\/])?claude(\.exe)?(?=\s|$)/i;
const CONTINUES = /(^|\s)(--resume|-r|--continue|-c)(=|\s|$)/;
const HAS_NAME = /(^|\s)(-n|--name)(=|\s|$)/;
const SESSION_ID_FLAG = /\s--session-id(=|\s+)\S+/g;

/**
 * 새로 띄우는 claude 에 세션 id 를 붙인다(--session-id) — 보드가 그 세션을 미리 안다.
 * claude 가 아니거나 이어 열기·이어 하기면 그대로(id 는 null). 이미 --session-id 가 있으면 새 id 로 바꾼다.
 */
export function freshSessionCommand(command: string): { command: string; sessionId: string | null } {
  if (!CLAUDE_START.test(command) || CONTINUES.test(command)) return { command, sessionId: null };
  const sessionId = crypto.randomUUID();
  return { command: command.replace(SESSION_ID_FLAG, "") + " --session-id " + sessionId, sessionId };
}

/** claude 명령이면 `--name <이름>` 을 claude 바로 뒤에 붙인다. claude 가 아니거나 사용자가 -n·--name 을 적었으면 그대로. 이름은 sessionNameArg 의 결과 */
export function namedCommand(command: string, nameArg: string): string {
  const start = CLAUDE_START.exec(command);
  if (!start || HAS_NAME.test(command)) return command;
  if (!isSessionNameArg(nameArg)) throw new Error("정리되지 않은 세션 이름");
  const at = start[0].length;
  return `${command.slice(0, at)} --name ${nameArg}${command.slice(at)}`;
}

/** 일시중지했던 세션을 이어 열 때 첫 요청으로 넘기는 문구(ClaudeBoard.swift carryOnPrompt) — 큰따옴표로 묶어 줄에 넣으므로 셸 특수문자를 담지 않는다 */
export const CARRY_ON_PROMPT = "보드에서 일시중지했던 세션을 재개합니다. 중단된 지점부터 직전 작업을 이어서 진행해 주세요.";

// 허용 목록 — 이 글자만 든 인자(맨 앞 @ 는 파일 첨부로 읽히므로 제외)는 cmd.exe·PowerShell 어느 쪽에서도 따옴표 없이 한 낱말로 읽힌다
const SAFE_ARG = /^(?!@)[A-Za-z0-9._:=/@+-]+$/;
const SESSION_ID = /^[0-9A-Za-z-]{8,64}$/;
// 이어 열기가 스스로 붙이는 것과 부딪히는 인자 — 있으면 통째로 기본 줄로 돌아간다(--name 은 namedCommand 가 붙인다)
const CONFLICTING = /^(--resume|-r|--continue|-c|--session-id|--name|-n)(=|$)/;

// 보드가 기록하는 resumeArgs 는 늘 `... --resume <id>` 로 끝난다 — 그 꼬리 쌍은 이어 열기가 스스로 붙이므로 떼고 나머지를 검사한다
function stripTrailingResume(args: string[]): string[] {
  const n = args.length;
  return n >= 2 && args[n - 2] === "--resume" ? args.slice(0, n - 2) : args;
}

// 보드가 띄운 세션은 늘 `--plugin-dir <...\plugins\md-render>` 를 달고 있다 — 이어 열 때 mdRenderCommand 가 다시 붙이므로 그 쌍은 떼고 나머지를 검사한다
// (백슬래시가 든 경로는 허용 목록을 못 넘어 원래 플래그까지 모두 버려지기 때문이다)
const MD_RENDER_DIR = /[\\/]plugins[\\/]md-render[\\/]?$/i;
function stripMdRenderPluginDir(args: string[]): string[] {
  const kept: string[] = [];
  for (let at = 0; at < args.length; at++) {
    const arg = args[at];
    if (arg === "--plugin-dir" && at + 1 < args.length && MD_RENDER_DIR.test(args[at + 1])) {
      at++;
      continue;
    }
    if (arg.startsWith("--plugin-dir=") && MD_RENDER_DIR.test(arg.slice("--plugin-dir=".length))) continue;
    kept.push(arg);
  }
  return kept;
}

/**
 * 끝난 세션을 보드 탭에서 이어 여는 명령줄(ClaudeBoard.swift resumeSession 의 Windows 판).
 * 옛 실행 인자(resumeArgs)는 허용 글자만 든 것을 남기고 -p·--print 는 뺀다(대화형으로 연다).
 * 끝의 `--resume <id>` 쌍은 먼저 뗀다. 남은 인자 하나라도 허용 밖이거나 --resume·--name 따위와 부딪히면 인자를 모두 버리고 `claude --resume <id>` 로 돌아간다.
 * 세션 id 가 모양에 안 맞으면 명령줄에 넣지 않고 던진다. carryOn 이면 CARRY_ON_PROMPT 를 큰따옴표로 묶어 뒤에 붙인다.
 */
export function resumeCommand(resumeArgs: string[], sessionId: string, carryOn: boolean): string {
  if (!SESSION_ID.test(sessionId)) throw new Error("세션 id 모양이 맞지 않는다");
  const args = stripMdRenderPluginDir(stripTrailingResume(resumeArgs));
  const trusted = args.every((arg) => SAFE_ARG.test(arg) && !CONFLICTING.test(arg));
  const kept = trusted ? args.filter((arg) => arg !== "-p" && arg !== "--print") : [];
  const line = ["claude", "--resume", sessionId, ...kept].join(" ");
  return carryOn ? `${line} "${CARRY_ON_PROMPT}"` : line;
}
