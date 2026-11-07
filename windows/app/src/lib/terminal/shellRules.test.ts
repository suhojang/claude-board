// @ts-nocheck — node:test 로만 돌고 svelte-check 대상이 아니다
// 실행: cd windows/app && node --test src/lib/terminal/shellRules.test.ts
import assert from "node:assert/strict";
import { test } from "node:test";
import { canTypeClaude, labelFor, normalizeShellKey, shellChoices } from "./shellRules.ts";

const ENVS = [
  { key: "pwsh", label: "PowerShell 7" },
  { key: "wsl:Ubuntu", label: "WSL · Ubuntu" },
];

// ── canTypeClaude 표: [키, 기대] ──
const TYPES: [string, boolean][] = [
  ["pwsh", true],
  ["powershell", true],
  ["cmd", true],
  ["git-bash", true],
  ["wsl", false],
  ["wsl:Ubuntu", false],
  ["wsl:Debian-12", false],
];
for (const [key, expected] of TYPES) {
  test(`canTypeClaude: ${key} -> ${expected}`, () => assert.equal(canTypeClaude(key), expected));
}

test("labelFor: 목록에 있으면 이름, 없으면 키 그대로", () => {
  assert.equal(labelFor("wsl:Ubuntu", ENVS), "WSL · Ubuntu");
  assert.equal(labelFor("wsl:Gone", ENVS), "wsl:Gone");
  assert.equal(labelFor("cmd", []), "cmd");
});

// ── normalizeShellKey 표: [입력, 기대] ──
const NORMALIZE: [unknown, string | null][] = [
  ["pwsh", "pwsh"],
  ["  git-bash ", "git-bash"],
  ["wsl", "wsl"],
  ["wsl:Ubuntu-22.04", "wsl:Ubuntu-22.04"],
  ["zsh", null],
  ["wsl:", null],
  ["wsl:bad name", null],
  ["wsl:a;b", null],
  ["", null],
  [null, null],
  [undefined, null],
  [3, null],
];
for (const [raw, expected] of NORMALIZE) {
  test(`normalizeShellKey: ${JSON.stringify(raw)} -> ${JSON.stringify(expected)}`, () => assert.equal(normalizeShellKey(raw), expected));
}

test("shellChoices: 자동(기본)이 맨 앞이고 뒤는 목록 순서 그대로", () => {
  assert.deepEqual(shellChoices(ENVS), [
    { key: null, label: "자동(기본)" },
    { key: "pwsh", label: "PowerShell 7" },
    { key: "wsl:Ubuntu", label: "WSL · Ubuntu" },
  ]);
  assert.deepEqual(shellChoices([]), [{ key: null, label: "자동(기본)" }]);
});
