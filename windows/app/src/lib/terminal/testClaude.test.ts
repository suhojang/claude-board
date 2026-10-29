// @ts-nocheck — node:test 로만 돌고 svelte-check 대상이 아니다
// 시험용 claude 바꿔치기(D5) 표 — 실행: cd windows/app && node --test src/lib/terminal/testClaude.test.ts
import assert from "node:assert/strict";
import { test } from "node:test";
import {
  applyTestClaude,
  currentTestClaudeMode,
  currentTestClaudePrefix,
  loadTestClaudePrefix,
  resetTestClaude,
  setTestClaudePrefix,
  typingBlockedReason,
} from "./testClaude.ts";

const FAKE = ["C:\\cbtest\\fake-bin\\claude.exe", "C:\\repo\\windows\\ci\\fake-claude-tty.cjs"];

test("시험 모드가 아니면(prefix 없음·빈 배열) 명령을 그대로 둔다", () => {
  for (const shell of ["pwsh", "powershell", "cmd"] as const) {
    assert.equal(applyTestClaude("claude --resume abc", null, shell), "claude --resume abc");
    assert.equal(applyTestClaude("claude --resume abc", [], shell), "claude --resume abc");
  }
});

test("pwsh·powershell — & 와 작은따옴표로 묶는다(앞쪽 인자도 조각마다)", () => {
  const want = "& 'C:\\cbtest\\fake-bin\\claude.exe' 'C:\\repo\\windows\\ci\\fake-claude-tty.cjs' --resume abc";
  assert.equal(applyTestClaude("claude --resume abc", FAKE, "pwsh"), want);
  assert.equal(applyTestClaude("claude --resume abc", FAKE, "powershell"), want);
});

test("cmd — 큰따옴표로 묶고 & 는 붙이지 않는다", () => {
  assert.equal(
    applyTestClaude("claude --resume abc", FAKE, "cmd"),
    '"C:\\cbtest\\fake-bin\\claude.exe" "C:\\repo\\windows\\ci\\fake-claude-tty.cjs" --resume abc',
  );
});

test("맨 앞 claude 낱말만 바꾼다 — claude.exe·claude.cmd·대소문자·경로 붙은 것·큰따옴표로 묶은 것", () => {
  const one = ["C:\\f\\claude.exe"];
  for (const lead of ["claude", "claude.exe", "claude.cmd", "CLAUDE.EXE", "C:\\Users\\x\\.local\\bin\\claude.exe", '"C:\\Program Files\\x\\claude.exe"', "./claude"]) {
    assert.equal(applyTestClaude(`${lead} -n hi`, one, "pwsh"), "& 'C:\\f\\claude.exe' -n hi", lead);
  }
});

test("앞 공백은 남기고, 인자로만 든 claude 는 건드리지 않는다", () => {
  assert.equal(applyTestClaude("  claude", ["C:\\f\\a.exe"], "cmd"), '  "C:\\f\\a.exe"');
  assert.equal(applyTestClaude("echo claude", ["C:\\f\\a.exe"], "cmd"), "echo claude");
  assert.equal(applyTestClaude("cd claude && claude", ["C:\\f\\a.exe"], "cmd"), "cd claude && claude");
});

test("claude 가 아닌 명령은 그대로 — claude2·claude-board·claude.bak·빈 줄·셸 명령", () => {
  for (const c of ["claude2 x", "claude-board --action pause", "claude.bak", "", "dir", "node claude.js", "cmd /c claude"]) {
    assert.equal(applyTestClaude(c, FAKE, "pwsh"), c, c);
  }
});

test("인자 안의 따옴표·claude 낱말은 그대로 둔다", () => {
  assert.equal(applyTestClaude('claude --name "claude 하나" claude', ["C:\\f\\a.exe"], "cmd"), '"C:\\f\\a.exe" --name "claude 하나" claude');
});

test("공백·한글·$ 가 든 경로 — pwsh 는 작은따옴표 안이라 글자 그대로", () => {
  assert.equal(applyTestClaude("claude", ["C:\\홍 길동\\$x\\a b.exe"], "pwsh"), "& 'C:\\홍 길동\\$x\\a b.exe'");
  assert.equal(applyTestClaude("claude", ["C:\\홍 길동\\a b.exe"], "cmd"), '"C:\\홍 길동\\a b.exe"');
});

test("작은따옴표(곡선 포함)가 든 경로 — PowerShell 은 두 번 써서 빠져나온다", () => {
  assert.equal(applyTestClaude("claude", ["C:\\it's\\a.exe"], "powershell"), "& 'C:\\it''s\\a.exe'");
  assert.equal(applyTestClaude("claude", ["C:\\it\u2019s\\a.exe"], "pwsh"), "& 'C:\\it\u2019\u2019s\\a.exe'");
});

test("받아 두는 값 — null 은 꺼짐, 조각 배열은 켜짐, 그 밖(빈 배열·문자열·모르는 모양)은 어긋남", () => {
  setTestClaudePrefix(FAKE);
  assert.deepEqual(currentTestClaudePrefix(), FAKE);
  assert.equal(currentTestClaudeMode().kind, "on");
  assert.equal(typingBlockedReason(), null);
  for (const off of [null, undefined]) {
    setTestClaudePrefix(FAKE);
    setTestClaudePrefix(off);
    assert.equal(currentTestClaudeMode().kind, "off");
    assert.equal(currentTestClaudePrefix(), null);
    assert.equal(typingBlockedReason(), null);
  }
  for (const bad of [[], [""], [1], "C:\\a.exe", { a: 1 }, 7]) {
    setTestClaudePrefix(FAKE);
    setTestClaudePrefix(bad);
    assert.equal(currentTestClaudeMode().kind, "invalid", JSON.stringify(bad));
    assert.equal(currentTestClaudePrefix(), null);
    assert.ok(typingBlockedReason(), JSON.stringify(bad));
  }
});

test("백엔드가 어긋남({invalid})을 알리면 쳐 넣기를 막는다 — 이유가 따라온다", () => {
  setTestClaudePrefix({ invalid: "CLAUDE_BOARD_CLAUDE 조각이 절대 경로가 아니다" });
  assert.equal(currentTestClaudeMode().kind, "invalid");
  assert.match(typingBlockedReason() ?? "", /절대 경로/);
  assert.equal(currentTestClaudePrefix(), null);
  setTestClaudePrefix({ invalid: "" });
  assert.ok(typingBlockedReason());
});

test("IPC 가 실패하면 닫힌 채로 — 진짜 claude 를 치지 않는다", async () => {
  await loadTestClaudePrefix(async () => FAKE);
  assert.deepEqual(currentTestClaudePrefix(), FAKE);
  await loadTestClaudePrefix(async () => {
    throw new Error("백엔드 없음");
  });
  assert.equal(currentTestClaudeMode().kind, "invalid");
  assert.match(typingBlockedReason() ?? "", /백엔드 없음/);
  await loadTestClaudePrefix(async () => null);
  assert.equal(currentTestClaudeMode().kind, "off");
  assert.equal(typingBlockedReason(), null);
});

test("아직 받기 전에는 친다고 보지 않는다 — 시험 모드인지 모르는 채로 치지 않는다", () => {
  resetTestClaude();
  assert.equal(currentTestClaudeMode().kind, "unloaded");
  assert.ok(typingBlockedReason());
});
