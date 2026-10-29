// @ts-nocheck — node:test 로만 돌고 svelte-check 대상이 아니다
// 실행: cd windows/app && node --test src/lib/terminal/mdRender.test.ts
// 실제 cmd.exe·PowerShell 이 줄을 읽는 대로의 시험은 mdRender.cmd.test.ts 에 있다.
import { test } from "node:test";
import assert from "node:assert/strict";
import { isMdRenderOff, mdRenderCommand, pruneMdRenderOff, quotePluginDir, rememberMdRenderOff } from "./mdRender.ts";

const DIR = String.raw`C:\Users\me\.claude\notifier\plugins\md-render`;
const SPACED = String.raw`C:\Users\홍 길동\.claude\notifier\plugins\md-render`;
const UUID = "8f3c2a10-1111-4222-8333-444455556666";

// ── mdRenderCommand 표: [이름, 명령, 폴더, 끔, 기대] ──
const table = [
  ["켜짐 — claude 낱말 바로 뒤에 하나", "claude --dangerously-skip-permissions", DIR, false, `claude --plugin-dir "${DIR}" --dangerously-skip-permissions`],
  ["켜짐 — 인자가 없어도", "claude", DIR, false, `claude --plugin-dir "${DIR}"`],
  ["켜짐 — 뒤의 --session-id 는 그대로", `claude --name proj --session-id ${UUID}`, DIR, false, `claude --plugin-dir "${DIR}" --name proj --session-id ${UUID}`],
  ["공백·한글 경로는 큰따옴표", "claude --resume abc", SPACED, false, `claude --plugin-dir "${SPACED}" --resume abc`],
  ["claude.exe·앞 경로도 claude", String.raw`C:\bin\claude.exe --resume abc`, DIR, false, String.raw`C:\bin\claude.exe --plugin-dir "${DIR}" --resume abc`],
  ["끔 — 붙이지 않는다", "claude --resume abc", DIR, true, "claude --resume abc"],
  ["폴더를 모르면 붙이지 않는다", "claude --resume abc", null, false, "claude --resume abc"],
  ["claude 가 아닌 명령은 그대로", "pwsh -NoLogo", DIR, false, "pwsh -NoLogo"],
  ["claude 가 아닌 명령은 그대로 — claudex", "claudex --x", DIR, false, "claudex --x"],
  ["빈 줄은 그대로", "", DIR, false, ""],
  ["플래그 둘 → 하나(값에 공백, 큰따옴표)", `claude --plugin-dir "${SPACED}" --resume abc --plugin-dir "${SPACED}"`, DIR, false, `claude --plugin-dir "${DIR}" --resume abc`],
  ["플래그 둘 → 하나(따옴표 없음·슬래시)", "claude --plugin-dir C:/old/plugins/md-render --resume abc --plugin-dir=D:/x/plugins/md-render", DIR, false, `claude --plugin-dir "${DIR}" --resume abc`],
  ["작은따옴표 꼴도 걷어 낸다", `claude --plugin-dir '${SPACED}' --resume abc`, DIR, false, `claude --plugin-dir "${DIR}" --resume abc`],
  ["옛 설치본 경로를 새 경로로 바꾼다", String.raw`claude --plugin-dir "C:\Users\me\AppData\Local\Programs\Claude Board\plugins\md-render" --resume abc`, DIR, false, `claude --plugin-dir "${DIR}" --resume abc`],
  ["끔 — 이미 든 것도 걷어 낸다", `claude --plugin-dir "${DIR}" --resume abc`, DIR, true, "claude --resume abc"],
  ["폴더를 모르면 이미 든 것은 걷어 낸다", `claude --resume abc --plugin-dir "${DIR}"`, null, false, "claude --resume abc"],
  ["대소문자·끝의 \\ 도 같은 경로", String.raw`claude --plugin-dir "C:\X\Plugins\MD-Render\" --resume abc`, DIR, false, `claude --plugin-dir "${DIR}" --resume abc`],
  ["사용자의 다른 --plugin-dir 은 둔다", "claude --plugin-dir C:/mine/plugins/other --resume abc", DIR, false, `claude --plugin-dir "${DIR}" --plugin-dir C:/mine/plugins/other --resume abc`],
  ["끔 — 다른 --plugin-dir 은 그대로", "claude --plugin-dir C:/mine/plugins/other", DIR, true, "claude --plugin-dir C:/mine/plugins/other"],
  ["md-render 로 끝나지 않는 이름은 둔다", String.raw`claude --plugin-dir C:\x\plugins\md-render-fork`, DIR, false, String.raw`claude --plugin-dir "${DIR}" --plugin-dir C:\x\plugins\md-render-fork`],
  ["carryOn 문구가 끝에 있어도 플래그는 앞", `claude --resume ${UUID} "다시 시작"`, DIR, false, `claude --plugin-dir "${DIR}" --resume ${UUID} "다시 시작"`],
];

for (const [name, command, dir, off, expected] of table) {
  test(`mdRenderCommand — ${name}`, () => {
    assert.equal(mdRenderCommand(command, dir, off), expected);
  });
}

test("mdRenderCommand — 같은 입력에 두 번 부르면 같은 결과(플래그가 늘지 않는다)", () => {
  for (const [, command, dir, off] of table) {
    const once = mdRenderCommand(command, dir, off);
    assert.equal(mdRenderCommand(once, dir, off), once, command);
  }
});

test("mdRenderCommand — 켠 줄에는 md-render 경로가 정확히 한 번", () => {
  for (const [, command, dir, off] of table) {
    if (off || dir === null || !/^\s*(\S*[\\/])?claude(\.exe)?(\s|$)/i.test(command)) continue;
    const found = mdRenderCommand(command, dir, off).match(/plugins[\\/]md-render(?![\w-])/gi) ?? [];
    assert.equal(found.length, 1, command);
  }
});

test("mdRenderCommand — 사용자가 다른 꼴로 적은 md-render 경로가 남으면 더 붙이지 않는다", () => {
  // 따옴표 한쪽만 있어 걷어 내는 꼴에 안 맞는다 — 둘이 되느니 사용자 것을 둔다
  const odd = String.raw`claude --plugin-dir "C:\a\plugins\md-render --resume abc`;
  assert.equal(mdRenderCommand(odd, DIR, false), odd);
});

test("mdRenderCommand — 셸이 따옴표 안에서도 풀어 버리는 글자가 든 폴더는 켠 줄에서 던진다", () => {
  for (const bad of ['C:\\a"b\\plugins\\md-render', "C:\\100%\\plugins\\md-render", "C:\\$env\\plugins\\md-render", "C:\\a`b\\plugins\\md-render", "C:\\a^b\\plugins\\md-render", "C:\\a!b\\plugins\\md-render", "C:\\\u201cx\\plugins\\md-render", "C:\\a\nb\\plugins\\md-render", "C:\\a\\plugins\\md-render\\", ""]) {
    assert.throws(() => mdRenderCommand("claude", bad, false), /안전하게/, JSON.stringify(bad));
    assert.throws(() => quotePluginDir(bad), /안전하게/, JSON.stringify(bad));
  }
  // 끄면 폴더를 쓰지 않으니 던지지 않는다
  assert.equal(mdRenderCommand("claude", 'C:\\a"b', true), "claude");
});

test("quotePluginDir — 공백·한글·& ( ) ; , ' 는 큰따옴표 안에서 안전하다", () => {
  for (const ok of [SPACED, String.raw`C:\a&b (x);y,z'w\plugins\md-render`, String.raw`D:\한글\plugins\md-render`]) {
    assert.equal(quotePluginDir(ok), `"${ok}"`);
  }
});

// ── 끈 세션 목록 ──

test("isMdRenderOff — 목록에 든 세션만 끈 것", () => {
  assert.equal(isMdRenderOff("a", ["a", "b"]), true);
  assert.equal(isMdRenderOff("c", ["a", "b"]), false);
  assert.equal(isMdRenderOff("a", []), false);
});

test("rememberMdRenderOff — 더한 새 목록, 이미 있으면 그대로, 원본은 바뀌지 않는다", () => {
  const list = ["a"];
  assert.deepEqual(rememberMdRenderOff(list, "b"), ["a", "b"]);
  assert.deepEqual(list, ["a"]);
  assert.deepEqual(rememberMdRenderOff(["a", "b"], "a"), ["a", "b"]);
  assert.deepEqual(rememberMdRenderOff([], "x"), ["x"]);
});

test("pruneMdRenderOff — 아직 있는 세션만 남긴다", () => {
  assert.deepEqual(pruneMdRenderOff(["a", "b", "c"], ["c", "a", "z"]), ["a", "c"]);
  assert.deepEqual(pruneMdRenderOff(["a"], []), []);
  assert.deepEqual(pruneMdRenderOff([], ["a"]), []);
});
