// 셸에 치는 claude 명령줄을 만드는 순수 함수 — launch.ts 에서 떼어 냈다. sessionName.ts 말고는 import 하지 않아 node:test 로 바로 시험한다.
// macOS 보드의 freshSessionCommand(ClaudeBoard.swift:6270-6277)와 named(5481-5500)에 맞춘 것이다.
import { isSessionNameArg } from "./sessionName.ts";

/** 새 세션 기본 명령 — 승인 대기가 거의 없게 권한 확인을 건너뛴다(ClaudeBoard.swift defaultSessionCommand) */
export const DEFAULT_SESSION_COMMAND = "claude --dangerously-skip-permissions";

// claude 로 시작하는 명령 — 앞의 경로(\ 또는 /)와 .exe 는 있어도 된다
const CLAUDE_START = /^\s*(\S*[\\/])?claude(\.exe)?(?=\s|$)/i;
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
