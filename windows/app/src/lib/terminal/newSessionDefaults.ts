// 새 세션의 기본값 — 설정 > 터미널의 기본 명령(newSessionCommand)과 답변 꾸미기 기본값(newSessionMdRender)을 새 세션 창과 경로 카드의 "+" 가 쓴다.
// sessionCommand.ts 말고는 import 하지 않는 순수 함수라 node:test 로 시험한다.
import { CLAUDE_START, DEFAULT_SESSION_COMMAND } from "./sessionCommand.ts";

/** 기본 명령 칸에 적을 수 있는 최대 글자 수 */
export const DEFAULT_COMMAND_MAX = 500;

/** 새 세션의 기본 명령 — 설정 값(앞뒤 공백 뺀 것), 비었거나 설정이 없거나 검사(validateDefaultCommand)에 걸리면 DEFAULT_SESSION_COMMAND.
 * 쓰는 길만 검사하면 손으로 고친 설정 파일의 줄바꿈·`& claude` 가 그대로 셸에 쳐진다 */
export function defaultNewSessionCommand(settings: { newSessionCommand?: unknown } | null | undefined): string {
  const typed = typeof settings?.newSessionCommand === "string" ? settings.newSessionCommand : "";
  const check = validateDefaultCommand(typed);
  return check.ok && check.command !== "" ? check.command : DEFAULT_SESSION_COMMAND;
}

/** 새 세션을 열 때 답변 꾸미기를 켜 두는가 — 설정이 없거나 불리언이 아니면 켬 */
export function defaultNewSessionMdRender(settings: { newSessionMdRender?: unknown } | null | undefined): boolean {
  return settings?.newSessionMdRender === false ? false : true;
}

export type DefaultCommandCheck = { ok: true; command: string } | { ok: false; reason: string };

/** 설정 칸에 친 기본 명령 검사 — 빈 칸은 통과(기본 명령으로 돌아간다). 그 밖은 claude 로 시작하고, 제어 문자가 없고, 길이 상한 안이어야 한다 */
export function validateDefaultCommand(text: string): DefaultCommandCheck {
  const command = text.trim();
  if (command === "") return { ok: true, command };
  // 줄바꿈·탭·NUL 등 — 셸에 쳐 넣을 때 명령이 여러 줄로 쪼개진다
  if (/[\u0000-\u001f\u007f]/.test(command)) return { ok: false, reason: "줄바꿈이나 제어 문자는 쓸 수 없습니다" };
  if (command.length > DEFAULT_COMMAND_MAX) return { ok: false, reason: `${DEFAULT_COMMAND_MAX}자까지 쓸 수 있습니다` };
  if (!CLAUDE_START.test(command)) return { ok: false, reason: "claude 로 시작해야 합니다" };
  return { ok: true, command };
}
