// @ts-nocheck — node:test 로만 돌고 svelte-check 대상이 아니다
// 자율주행 시작 준비 화면의 순수 부분(입력 검사·출발 요청 만들기)을 시험한다.
// 실행: cd windows/app && node --test src/lib/autopilot/startForm.test.ts
import assert from "node:assert/strict";
import { test } from "node:test";

import {
  MAX_UNITS,
  buildExecEnv,
  buildStart,
  buildUnit,
  emptyDraft,
  emptyUnit,
  folderName,
  makeRunId,
  needsConsent,
  parseCap,
  problemFor,
  slug,
  syntaxHint,
  validateDraft,
} from "./startForm.ts";

const WSL = { key: "wsl:Ubuntu-24.04", label: "WSL · Ubuntu-24.04", kind: "wsl", distro: "Ubuntu-24.04", sandboxCapable: true };
const PWSH = { key: "pwsh", label: "PowerShell 7", kind: "pwsh", distro: null, sandboxCapable: false };
const CMD = { key: "cmd", label: "명령 프롬프트", kind: "cmd", distro: null, sandboxCapable: false };
const ENVS = [WSL, PWSH, CMD];

const unit = (patch = {}) => ({ ...emptyUnit(), title: "목록 화면", goal: "할 일 목록 화면을 만든다.", check: "npm test", ...patch });
const draft = (patch = {}) => ({ ...emptyDraft(), projectDir: "C:\\work\\toy", envKey: "pwsh", consent: true, units: [unit()], ...patch });
const at = new Date(2026, 10, 6, 9, 5, 3);

test("폴더 이름은 마지막 조각이고 끝 구분자와 슬래시 방향을 가리지 않는다", () => {
  assert.equal(folderName("C:\\work\\toy"), "toy");
  assert.equal(folderName("C:\\work\\toy\\"), "toy");
  assert.equal(folderName("C:/work/toy//"), "toy");
  assert.equal(folderName("\\\\wsl.localhost\\Ubuntu\\home\\me\\app"), "app");
  assert.equal(folderName(""), "");
});

test("이름 조각은 실행 id 규칙 글자만 남기고 비면 run", () => {
  assert.equal(slug("my app"), "my-app");
  assert.equal(slug("프로젝트"), "run");
  assert.equal(slug("...a..."), "a");
  assert.equal(slug("a/b\\c"), "a-b-c");
  assert.equal(slug("x".repeat(100)).length, 40);
  assert.equal(slug("a".repeat(39) + "-bbb"), "a".repeat(39));
});

test("실행 id 는 지역 시각과 폴더 이름이고 실행 id 규칙(64자 이하)을 지킨다", () => {
  assert.equal(makeRunId(at, "C:\\work\\toy"), "20261106-090503-toy");
  assert.equal(makeRunId(at, "C:\\work\\한글"), "20261106-090503-run");
  const id = makeRunId(at, "C:\\" + "k".repeat(200));
  assert.match(id, /^[A-Za-z0-9._-]{1,64}$/);
});

test("상한 칸: 빈 칸은 기본값, 0보다 큰 수만 받는다", () => {
  assert.equal(parseCap(""), null);
  assert.equal(parseCap("  "), null);
  assert.equal(parseCap("30"), 30);
  assert.equal(parseCap("1.5"), 1.5);
  for (const bad of ["0", "-3", "abc", "1e3", "1,5", ".5", "5.", "Infinity", "0.0"]) assert.equal(parseCap(bad), "bad", bad);
});

test("샌드박스를 못 거는 환경만 동의가 필요하다", () => {
  assert.equal(needsConsent(WSL), false);
  assert.equal(needsConsent(PWSH), true);
  assert.equal(needsConsent(null), false);
});

test("처음 상태는 환경이 비어 출발할 수 없다", () => {
  const problems = validateDraft({ ...emptyDraft(), projectDir: "C:\\w", units: [unit()] }, ENVS);
  assert.deepEqual(problems.map((p) => p.field), ["env"]);
  assert.match(problemFor(problems, "env"), /기본값은 없습니다/);
});

test("네이티브 환경은 동의 없이는 막히고 WSL 은 동의가 필요 없다", () => {
  assert.deepEqual(validateDraft(draft({ consent: false }), ENVS).map((p) => p.field), ["consent"]);
  assert.deepEqual(validateDraft(draft({ consent: true }), ENVS), []);
  assert.deepEqual(validateDraft(draft({ envKey: WSL.key, consent: false }), ENVS), []);
});

test("이 PC 목록에 없는 환경 키는 거절한다", () => {
  assert.deepEqual(validateDraft(draft({ envKey: "git-bash" }), ENVS).map((p) => p.field), ["env"]);
});

test("프로젝트 폴더·제목·목표·판정 명령이 비면 칸마다 문제를 낸다", () => {
  const problems = validateDraft(draft({ projectDir: " ", units: [unit({ title: "", goal: " ", check: "" })] }), ENVS);
  assert.deepEqual(problems.map((p) => p.field), ["projectDir", "unit.1.title", "unit.1.goal", "unit.1.check"]);
});

test("단위 번호는 1부터이고 문제 글에 번호가 든다", () => {
  const problems = validateDraft(draft({ units: [unit(), unit({ check: "" })] }), ENVS);
  assert.equal(problems.length, 1);
  assert.equal(problems[0].field, "unit.2.check");
  assert.match(problems[0].message, /^단위 2:/);
});

test("상한 칸이 숫자가 아니면 문제, 비면 괜찮다", () => {
  const problems = validateDraft(draft({ units: [unit({ minutes: "abc", usd: "0" })] }), ENVS);
  assert.deepEqual(problems.map((p) => p.field), ["unit.1.minutes", "unit.1.usd"]);
  assert.deepEqual(validateDraft(draft({ units: [unit({ minutes: "", usd: " " })] }), ENVS), []);
});

test("모델은 비면 안 되고 이름 꼴을 지킨다", () => {
  assert.deepEqual(validateDraft(draft({ model: " " }), ENVS).map((p) => p.field), ["model"]);
  assert.deepEqual(validateDraft(draft({ model: "Claude Sonnet" }), ENVS).map((p) => p.field), ["model"]);
  assert.deepEqual(validateDraft(draft({ model: "claude-sonnet-5-5" }), ENVS), []);
  assert.deepEqual(validateDraft(draft({ model: "opus" }), ENVS), []);
  assert.deepEqual(validateDraft(draft({ model: "sonnet[1m]" }), ENVS), []);
});

test("단위가 없거나 너무 많으면 문제", () => {
  assert.deepEqual(validateDraft(draft({ units: [] }), ENVS).map((p) => p.field), ["units"]);
  const many = Array.from({ length: MAX_UNITS + 1 }, () => unit());
  assert.deepEqual(validateDraft(draft({ units: many }), ENVS).map((p) => p.field), ["units"]);
});

test("단위는 키가 이름 순이고 작업 폴더를 비우면 프로젝트 폴더를 쓴다", () => {
  const built = buildUnit(unit({ title: " 제목 ", goal: " 목표 ", check: " exit 0 " }), 1, "C:\\work\\toy");
  assert.deepEqual(built, { check: "exit 0", cwd: "C:\\work\\toy", id: "u1", prompt: "목표", title: "제목" });
  assert.deepEqual(Object.keys(built), ["check", "cwd", "id", "prompt", "title"]);
});

test("단위의 작업 폴더와 상한은 쓴 대로 간다", () => {
  const built = buildUnit(unit({ cwd: " C:\\work\\toy\\api ", minutes: "45", usd: "2.5" }), 3, "C:\\work\\toy");
  assert.equal(built.cwd, "C:\\work\\toy\\api");
  assert.equal(built.id, "u3");
  assert.deepEqual(built.caps, { minutes: 45, usd: 2.5 });
  assert.deepEqual(Object.keys(built), ["caps", "check", "cwd", "id", "prompt", "title"]);
});

test("상한을 하나만 쓰면 그 키만 caps 에 든다", () => {
  assert.deepEqual(buildUnit(unit({ usd: "3" }), 1, "C:\\p").caps, { usd: 3 });
  assert.deepEqual(buildUnit(unit({ minutes: "10" }), 1, "C:\\p").caps, { minutes: 10 });
  assert.equal("caps" in buildUnit(unit(), 1, "C:\\p"), false);
});

test("실행 환경 기록: 네이티브는 key·kind, WSL 은 distro 가 더 든다. 동의 기록은 쓰지 않는다", () => {
  assert.deepEqual(buildExecEnv(PWSH), { key: "pwsh", kind: "pwsh" });
  assert.deepEqual(buildExecEnv(WSL), { distro: "Ubuntu-24.04", key: "wsl:Ubuntu-24.04", kind: "wsl" });
  assert.equal("unsandboxed_ok" in buildExecEnv(PWSH), false);
});

test("네이티브 + 동의 → run.json·queue.json·--accept-unsandboxed", () => {
  const result = buildStart(draft({ title: "야간 작업", model: " sonnet ", units: [unit(), unit({ title: "둘째", goal: "정렬", check: "npm run lint", cwd: "C:\\work\\toy\\web", minutes: "20" })] }), ENVS, at);
  assert.equal(result.ok, true);
  const r = result.request;
  assert.equal(r.runId, "20261106-090503-toy");
  assert.equal(r.acceptUnsandboxed, true);
  assert.deepEqual(r.run, {
    exec_env: { key: "pwsh", kind: "pwsh" },
    model: "sonnet",
    project: "toy",
    project_root: "C:\\work\\toy",
    title: "야간 작업",
  });
  assert.deepEqual(r.queue, [
    { check: "npm test", cwd: "C:\\work\\toy", id: "u1", prompt: "할 일 목록 화면을 만든다.", title: "목록 화면" },
    { caps: { minutes: 20 }, check: "npm run lint", cwd: "C:\\work\\toy\\web", id: "u2", prompt: "정렬", title: "둘째" },
  ]);
});

test("WSL 은 동의 표시를 켜 둬도 --accept-unsandboxed 를 붙이지 않는다", () => {
  const result = buildStart(draft({ envKey: WSL.key, consent: true }), ENVS, at);
  assert.equal(result.ok, true);
  assert.equal(result.request.acceptUnsandboxed, false);
  assert.deepEqual(result.request.run.exec_env, { distro: "Ubuntu-24.04", key: "wsl:Ubuntu-24.04", kind: "wsl" });
});

test("실행 이름을 비우면 프로젝트 폴더 이름이 제목이 된다", () => {
  const result = buildStart(draft({ title: "  " }), ENVS, at);
  assert.equal(result.request.run.title, "toy");
});

test("검사에 걸리면 요청을 만들지 않고 문제를 돌려준다", () => {
  const result = buildStart(draft({ consent: false }), ENVS, at);
  assert.equal(result.ok, false);
  assert.deepEqual(result.problems.map((p) => p.field), ["consent"]);
});

test("run.json 에는 엔진 기본값을 베끼지 않는다(멈출 선·상한은 비운 채)", () => {
  const { request } = buildStart(draft(), ENVS, at);
  assert.deepEqual(Object.keys(request.run), ["exec_env", "model", "project", "project_root", "title"]);
});

test("판정 명령 문법 안내는 고른 환경을 따른다", () => {
  assert.match(syntaxHint(null), /고르면/);
  assert.match(syntaxHint(PWSH), /^PowerShell/);
  assert.match(syntaxHint(CMD), /^명령 프롬프트/);
  assert.match(syntaxHint({ ...PWSH, kind: "git-bash" }), /^bash.*Git Bash/);
  assert.equal(syntaxHint(WSL), "bash 문법으로 씁니다. 작업 폴더는 \\\\wsl.localhost\\Ubuntu-24.04\\ 아래 경로여야 합니다.");
});
