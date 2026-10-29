// @ts-nocheck — node:test 로만 돌고 svelte-check 대상이 아니다
// 실행: cd windows/app && node --test src/lib/terminal/newSessionDefaults.test.ts
import { test } from "node:test";
import assert from "node:assert/strict";
import {
  DEFAULT_COMMAND_MAX,
  defaultNewSessionCommand,
  defaultNewSessionMdRender,
  validateDefaultCommand,
} from "./newSessionDefaults.ts";
import { DEFAULT_SESSION_COMMAND } from "./sessionCommand.ts";

// ── defaultNewSessionCommand 표: [이름, 설정, 기대] ──
const COMMANDS: [string, unknown, string][] = [
  ["빈 문자열 -> 기본 명령", { newSessionCommand: "" }, DEFAULT_SESSION_COMMAND],
  ["공백만 -> 기본 명령", { newSessionCommand: "  \t " }, DEFAULT_SESSION_COMMAND],
  ["필드 없음 -> 기본 명령", {}, DEFAULT_SESSION_COMMAND],
  ["설정 없음(null) -> 기본 명령", null, DEFAULT_SESSION_COMMAND],
  ["설정 없음(undefined) -> 기본 명령", undefined, DEFAULT_SESSION_COMMAND],
  ["문자열이 아니면 -> 기본 명령", { newSessionCommand: 3 }, DEFAULT_SESSION_COMMAND],
  ["값이 있으면 앞뒤 공백을 뺀 그대로", { newSessionCommand: "  claude --model opus " }, "claude --model opus"],
  // 손으로 고친 설정 파일 — 읽는 길도 검사해 셸에 위험한 글자가 들어가지 않게 한다
  ["줄바꿈이 든 값 -> 기본 명령", { newSessionCommand: "claude\nrm -rf x" }, DEFAULT_SESSION_COMMAND],
  ["claude 로 시작하지 않으면 -> 기본 명령", { newSessionCommand: "& claude" }, DEFAULT_SESSION_COMMAND],
  ["길이 상한을 넘으면 -> 기본 명령", { newSessionCommand: `claude ${"a".repeat(600)}` }, DEFAULT_SESSION_COMMAND],
];
for (const [name, settings, want] of COMMANDS) {
  test(`defaultNewSessionCommand: ${name}`, () => assert.equal(defaultNewSessionCommand(settings), want));
}

// ── defaultNewSessionMdRender 표 ──
const MD: [string, unknown, boolean][] = [
  ["필드 없음 -> 켬", {}, true],
  ["설정 없음 -> 켬", null, true],
  ["true", { newSessionMdRender: true }, true],
  ["false -> 끔", { newSessionMdRender: false }, false],
  ["불리언이 아니면 -> 켬", { newSessionMdRender: "false" }, true],
];
for (const [name, settings, want] of MD) {
  test(`defaultNewSessionMdRender: ${name}`, () => assert.equal(defaultNewSessionMdRender(settings), want));
}

// ── validateDefaultCommand 표: [이름, 입력, 기대 ok, 기대 command(ok 일 때)] ──
const VALID: [string, string, boolean, string?][] = [
  ["빈 칸은 통과(기본 명령으로 돌아감)", "", true, ""],
  ["공백만도 빈 칸", "   ", true, ""],
  ["claude 만", "claude", true, "claude"],
  ["옵션이 붙은 claude", "claude --model opus", true, "claude --model opus"],
  ["앞뒤 공백은 뺀다", "  claude -c  ", true, "claude -c"],
  ["경로가 앞에 있어도 claude", String.raw`C:\tools\claude.exe --verbose`, true, String.raw`C:\tools\claude.exe --verbose`],
  ["대소문자 무시", "Claude", true, "Claude"],
  ["claude 로 시작하지 않으면 거부", "echo claude", false],
  ["claude 에 글자가 붙은 다른 이름은 거부", "claudex --foo", false],
  ["셸 명령은 거부", "powershell", false],
  ["줄바꿈이 있으면 거부", "claude\nrm -rf x", false],
  ["캐리지리턴이 있으면 거부", "claude\r--foo", false],
  ["탭이 있으면 거부", "claude\t--foo", false],
  ["NUL 이 있으면 거부", "claude\u0000", false],
  ["DEL 이 있으면 거부", "claude\u007f", false],
  ["상한 길이는 통과", "claude " + "a".repeat(DEFAULT_COMMAND_MAX - 7), true],
  ["상한을 넘으면 거부", "claude " + "a".repeat(DEFAULT_COMMAND_MAX - 6), false],
];
for (const [name, text, ok, command] of VALID) {
  test(`validateDefaultCommand: ${name}`, () => {
    const r = validateDefaultCommand(text);
    assert.equal(r.ok, ok);
    if (r.ok) assert.equal(r.command, command ?? text);
    else assert.ok(r.reason.length > 0, "거부 사유가 있어야 한다");
  });
}
