// @ts-nocheck — node:test 로만 돌고 svelte-check 대상이 아니다
// 자율주행 시작 준비 화면의 상태 기계(start.svelte.ts)를 시험한다: 열고 닫기, 출발, 이중 클릭, 거절 글, 동의 표시.
// start.svelte.ts 는 `$state` 룬과 `$lib/backend` 별칭을 쓰므로 node 가 그냥 읽지 못한다. 이 시험은 `module.registerHooks` 로
//   - `.svelte.ts` 를 svelte 의 compileModule 로 바꿔 읽고(타입은 node:module 의 stripTypeScriptTypes 로 먼저 뗀다),
//   - `$lib/backend` 를 `globalThis.__backend` 로 돌려주는 가짜 모듈로 바꾼다.
// 새 의존성은 없다(svelte 는 이미 devDependency). 컴포넌트(.svelte)는 이 방식으로 읽지 못한다 — 화면 그리기·키 입력 차단은 시험하지 않는다.
// 실행: cd windows/app && node --test src/lib/autopilot/startState.test.ts
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { registerHooks, stripTypeScriptTypes } from "node:module";
import { beforeEach, test } from "node:test";
import { fileURLToPath } from "node:url";

import { compileModule } from "svelte/compiler";

const BACKEND_URL = "cb-test:backend";

registerHooks({
  resolve(specifier, context, nextResolve) {
    if (specifier === "$lib/backend") return { url: BACKEND_URL, shortCircuit: true };
    // start.svelte.ts 가 확장자 없이 쓴 같은 폴더 모듈
    if (specifier === "./startForm") return nextResolve("./startForm.ts", context);
    return nextResolve(specifier, context);
  },
  load(url, context, nextLoad) {
    if (url === BACKEND_URL) {
      const source = "export const autopilotEnvs = (...a) => globalThis.__backend.autopilotEnvs(...a);\nexport const autopilotStart = (...a) => globalThis.__backend.autopilotStart(...a);\n";
      return { format: "module", source, shortCircuit: true };
    }
    if (url.endsWith(".svelte.ts")) {
      const ts = readFileSync(fileURLToPath(url), "utf8");
      const js = stripTypeScriptTypes(ts);
      const { js: out } = compileModule(js, { generate: "client", filename: fileURLToPath(url) });
      return { format: "module", source: out.code, shortCircuit: true };
    }
    return nextLoad(url, context);
  },
});

const WSL = { key: "wsl:Ubuntu-24.04", label: "WSL", kind: "wsl", distro: "Ubuntu-24.04", sandboxCapable: true };
const PWSH = { key: "pwsh", label: "PowerShell 7", kind: "pwsh", distro: null, sandboxCapable: false };

const form = await import("./startForm.ts");
const ui = await import("./start.svelte.ts");
const { AutopilotStart } = ui;

/** 결정할 수 있는 약속 */
function deferred() {
  let resolve, reject;
  const promise = new Promise((res, rej) => ((resolve = res), (reject = rej)));
  return { promise, resolve, reject };
}

let startCalls;
let envCalls;
let startImpl;
let envsImpl;

const filledDraft = (patch = {}) => ({
  ...form.emptyDraft(),
  projectDir: "C:\\work\\toy",
  envKey: "pwsh",
  consent: true,
  units: [{ ...form.emptyUnit(), title: "제목", goal: "목표", check: "exit 0" }],
  ...patch,
});

beforeEach(() => {
  startCalls = [];
  envCalls = 0;
  startImpl = async () => ({ runId: "x", scheduled: true });
  envsImpl = async () => [WSL, PWSH];
  globalThis.__backend = {
    autopilotStart: (req) => {
      startCalls.push(req);
      return startImpl(req);
    },
    autopilotEnvs: () => {
      envCalls += 1;
      return envsImpl();
    },
  };
  AutopilotStart.shown = false;
  AutopilotStart.draft = filledDraft();
  AutopilotStart.envs = [WSL, PWSH];
  AutopilotStart.envsLoaded = true;
  AutopilotStart.tried = false;
  AutopilotStart.starting = false;
  AutopilotStart.error = "";
});

// 마이크로태스크를 비운다
const settle = () => new Promise((r) => setImmediate(r));

test("열면 보이고 이전 거절 글은 지워지며 실행 환경 목록을 새로 받는다", async () => {
  AutopilotStart.error = "지난번 거절";
  AutopilotStart.envs = [];
  AutopilotStart.envsLoaded = false;
  ui.openAutopilotStart();
  assert.equal(AutopilotStart.shown, true);
  assert.equal(AutopilotStart.error, "");
  await settle();
  assert.equal(envCalls, 1);
  assert.deepEqual(AutopilotStart.envs.map((e) => e.key), ["wsl:Ubuntu-24.04", "pwsh"]);
  assert.equal(AutopilotStart.envsLoaded, true);
  ui.openAutopilotStart();
  await settle();
  assert.equal(envCalls, 2, "열 때마다 다시 받는다(WSL 을 새로 깔았을 수 있다)");
});

test("실행 환경 목록을 못 받으면 빈 목록으로 두고 받았다고 표시한다(화면이 영원히 기다리지 않게)", async () => {
  const warn = console.warn;
  console.warn = () => {};
  try {
    envsImpl = async () => {
      throw new Error("백엔드 없음");
    };
    ui.openAutopilotStart();
    await settle();
    assert.deepEqual(AutopilotStart.envs, []);
    assert.equal(AutopilotStart.envsLoaded, true);
    assert.equal(AutopilotStart.shown, true);
  } finally {
    console.warn = warn;
  }
});

test("닫아도 쓰던 칸은 남는다", () => {
  AutopilotStart.draft.title = "야간 작업";
  ui.openAutopilotStart();
  ui.closeAutopilotStart();
  assert.equal(AutopilotStart.shown, false);
  assert.equal(AutopilotStart.draft.title, "야간 작업");
  assert.equal(AutopilotStart.draft.units[0].goal, "목표");
});

test("칸 검사에 걸리면 백엔드를 부르지 않고, 문제 글을 보이도록 tried 를 켜고, 열어 둔다", async () => {
  ui.openAutopilotStart();
  AutopilotStart.draft.units[0].check = "  ";
  await ui.submitStart();
  assert.equal(startCalls.length, 0);
  assert.equal(AutopilotStart.tried, true);
  assert.equal(AutopilotStart.starting, false);
  assert.equal(AutopilotStart.shown, true);
  assert.equal(AutopilotStart.error, "");
});

test("출발에 성공하면 요청을 보내고, 화면을 닫고, 칸을 비운다", async () => {
  ui.openAutopilotStart();
  await ui.submitStart();
  assert.equal(startCalls.length, 1);
  const req = startCalls[0];
  assert.match(req.runId, /^\d{8}-\d{6}-toy$/);
  assert.equal(req.acceptUnsandboxed, true);
  assert.equal(req.queue.length, 1);
  assert.equal(AutopilotStart.shown, false);
  assert.equal(AutopilotStart.starting, false);
  assert.equal(AutopilotStart.tried, false);
  assert.deepEqual(AutopilotStart.draft, form.emptyDraft(), "다음에 열면 빈 칸이다");
});

test("거절당하면 글을 그대로(여러 줄 포함) 남기고, 화면은 열려 있고, 칸은 그대로다", async () => {
  const cases = [
    new Error("단위 1: 작업 폴더가 없습니다.\n둘째 줄\n  들여쓴 셋째 줄 "),
    new Error(""),
    new Error("한글 \u{1F600} <b>태그</b> & 기호"),
  ];
  for (const err of cases) {
    AutopilotStart.draft = filledDraft();
    AutopilotStart.shown = true;
    startImpl = async () => {
      throw err;
    };
    await ui.submitStart();
    assert.equal(AutopilotStart.error, err.message, "Error 메시지를 고치지 않고 보인다");
    assert.equal(AutopilotStart.shown, true);
    assert.equal(AutopilotStart.starting, false, "실패해도 다시 누를 수 있다");
    assert.equal(AutopilotStart.draft.units[0].goal, "목표", "쓴 칸이 날아가지 않는다");
    assert.equal(AutopilotStart.tried, true);
  }
});

test("Error 가 아닌 거절(글자 하나)도 그 글을 보인다", async () => {
  startImpl = async () => {
    throw "그냥 글자";
  };
  await ui.submitStart();
  assert.equal(AutopilotStart.error, "그냥 글자");
});

test("거절 뒤에 다시 눌러 성공하면 거절 글이 사라진다", async () => {
  startImpl = async () => {
    throw new Error("첫 시도 거절");
  };
  await ui.submitStart();
  assert.equal(AutopilotStart.error, "첫 시도 거절");
  startImpl = async () => ({ runId: "x", scheduled: false });
  await ui.submitStart();
  assert.equal(AutopilotStart.error, "");
  assert.equal(AutopilotStart.shown, false);
  assert.equal(startCalls.length, 2);
});

test("출발 중에 또 누르면 백엔드를 두 번 부르지 않는다(이중 클릭)", async () => {
  const d = deferred();
  startImpl = () => d.promise;
  const first = ui.submitStart();
  const second = ui.submitStart();
  const third = ui.submitStart();
  await settle();
  assert.equal(AutopilotStart.starting, true);
  assert.equal(startCalls.length, 1, "한 번만 출발한다");
  d.resolve({ runId: "x", scheduled: true });
  await Promise.all([first, second, third]);
  assert.equal(startCalls.length, 1);
  assert.equal(AutopilotStart.starting, false);
  assert.equal(AutopilotStart.shown, false);
});

test("출발 중인 요청이 거절로 끝나면 그 사이 눌렀던 것은 사라지고 다시 누를 수 있다", async () => {
  const d = deferred();
  startImpl = () => d.promise;
  const first = ui.submitStart();
  const dup = ui.submitStart();
  d.reject(new Error("거절"));
  await Promise.all([first, dup]);
  assert.equal(startCalls.length, 1);
  assert.equal(AutopilotStart.error, "거절");
  startImpl = async () => ({ runId: "x", scheduled: true });
  await ui.submitStart();
  assert.equal(startCalls.length, 2);
});

test("출발 중에 칸 검사를 통과 못 하는 상태로 눌러도 진행 중인 출발을 건드리지 않는다", async () => {
  const d = deferred();
  startImpl = () => d.promise;
  const first = ui.submitStart();
  AutopilotStart.draft.units[0].check = "";
  await ui.submitStart();
  assert.equal(AutopilotStart.starting, true, "진행 중 표시가 꺼지지 않는다");
  d.resolve({ runId: "x", scheduled: true });
  await first;
  assert.equal(startCalls.length, 1);
});

test("환경을 바꾸면 동의 표시가 내려가고, 같은 환경을 다시 골라도 내려가지 않는다", () => {
  AutopilotStart.draft.envKey = "pwsh";
  AutopilotStart.draft.consent = true;
  ui.chooseEnv("pwsh");
  assert.equal(AutopilotStart.draft.consent, true, "같은 환경이면 그대로");
  ui.chooseEnv("wsl:Ubuntu-24.04");
  assert.equal(AutopilotStart.draft.envKey, "wsl:Ubuntu-24.04");
  assert.equal(AutopilotStart.draft.consent, false, "다른 환경에 대한 동의가 따라가지 않는다");
  AutopilotStart.draft.consent = true;
  ui.chooseEnv("pwsh");
  assert.equal(AutopilotStart.draft.consent, false);
});

test("동의했던 네이티브에서 WSL 로 바꾼 뒤에는 동의 없이 출발하고 --accept-unsandboxed 가 붙지 않는다", async () => {
  ui.chooseEnv("wsl:Ubuntu-24.04");
  await ui.submitStart();
  assert.equal(startCalls.length, 1);
  assert.equal(startCalls[0].acceptUnsandboxed, false);
});

test("동의했던 WSL 에서 네이티브로 바꾸면 다시 동의하기 전까지 출발하지 못한다", async () => {
  AutopilotStart.draft.envKey = "wsl:Ubuntu-24.04";
  ui.chooseEnv("pwsh");
  await ui.submitStart();
  assert.equal(startCalls.length, 0);
  assert.equal(AutopilotStart.tried, true);
});

test("단위는 상한(MAX_UNITS)까지만 더해지고 빼면 줄어든다", () => {
  AutopilotStart.draft.units = [form.emptyUnit()];
  for (let i = 0; i < form.MAX_UNITS + 10; i++) ui.addUnit();
  assert.equal(AutopilotStart.draft.units.length, form.MAX_UNITS);
  ui.removeUnit(0);
  assert.equal(AutopilotStart.draft.units.length, form.MAX_UNITS - 1);
  ui.addUnit();
  assert.equal(AutopilotStart.draft.units.length, form.MAX_UNITS);
});

test("고른 환경이 새 목록에 없으면(다시 열었더니 WSL 이 사라짐) 출발하지 못한다", async () => {
  AutopilotStart.draft.envKey = "wsl:Ubuntu-24.04";
  AutopilotStart.envs = [PWSH];
  await ui.submitStart();
  assert.equal(startCalls.length, 0);
});
