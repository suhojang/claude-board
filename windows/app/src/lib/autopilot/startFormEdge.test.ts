// @ts-nocheck — node:test 로만 돌고 svelte-check 대상이 아니다
// 자율주행 시작 준비 화면의 순수 부분(startForm.ts)의 경계·가장자리: 개수·길이 상한, 공백뿐인 칸, 한글 폴더 이름, 상한 칸 표, 동의 표, 모델 이름 표.
// 기본 동작은 startForm.test.ts 가 본다. 실행: cd windows/app && node --test src/lib/autopilot/startFormEdge.test.ts
import assert from "node:assert/strict";
import { test } from "node:test";

import {
  MAX_CHECK,
  MAX_GOAL,
  MAX_TITLE,
  MAX_UNITS,
  MODEL_PATTERN,
  buildStart,
  buildUnit,
  emptyDraft,
  emptyUnit,
  folderName,
  makeRunId,
  parseCap,
  problemFor,
  validateDraft,
} from "./startForm.ts";

const WSL = { key: "wsl:Ubuntu-24.04", label: "WSL", kind: "wsl", distro: "Ubuntu-24.04", sandboxCapable: true };
const PWSH = { key: "pwsh", label: "PowerShell 7", kind: "pwsh", distro: null, sandboxCapable: false };
const ENVS = [WSL, PWSH];

const unit = (patch = {}) => ({ ...emptyUnit(), title: "목록 화면", goal: "할 일 목록 화면을 만든다.", check: "npm test", ...patch });
const draft = (patch = {}) => ({ ...emptyDraft(), projectDir: "C:\\work\\toy", envKey: "pwsh", consent: true, units: [unit()], ...patch });
const at = new Date(2026, 10, 6, 9, 5, 3);
const fields = (problems) => problems.map((p) => p.field);

test("단위 수 경계: 1개와 30개는 통과, 0개와 31개는 거절", () => {
  const n = (count) => Array.from({ length: count }, () => unit());
  assert.deepEqual(validateDraft(draft({ units: n(1) }), ENVS), []);
  assert.deepEqual(validateDraft(draft({ units: n(MAX_UNITS) }), ENVS), []);
  assert.deepEqual(fields(validateDraft(draft({ units: n(MAX_UNITS + 1) }), ENVS)), ["units"]);
  assert.deepEqual(fields(validateDraft(draft({ units: n(0) }), ENVS)), ["units"]);
  assert.equal(MAX_UNITS, 30, "백엔드(autopilot.rs MAX_UNITS)와 같은 값");
});

test("단위 30개를 만들면 번호가 u1..u30 이고 서로 겹치지 않는다", () => {
  const units = Array.from({ length: MAX_UNITS }, (_, i) => unit({ title: `단위 ${i}` }));
  const r = buildStart(draft({ units }), ENVS, at);
  assert.equal(r.ok, true);
  assert.deepEqual(r.request.queue.map((q) => q.id), Array.from({ length: MAX_UNITS }, (_, i) => `u${i + 1}`));
});

// [칸 이름, 상한, 칸을 채우는 함수, 문제가 나는 칸]
const LIMITS = [
  ["단위 제목", MAX_TITLE, (s) => ({ units: [unit({ title: s })] }), "unit.1.title"],
  ["목표", MAX_GOAL, (s) => ({ units: [unit({ goal: s })] }), "unit.1.goal"],
  ["판정 명령", MAX_CHECK, (s) => ({ units: [unit({ check: s })] }), "unit.1.check"],
  ["실행 이름", MAX_TITLE, (s) => ({ title: s }), "title"],
];

test("글 칸 길이 경계: 상한 글자는 통과, 한 글자 넘으면 그 칸만 거절", () => {
  for (const [name, max, patch, field] of LIMITS) {
    assert.deepEqual(validateDraft(draft(patch("가".repeat(max))), ENVS), [], `${name} ${max}자`);
    assert.deepEqual(fields(validateDraft(draft(patch("가".repeat(max + 1))), ENVS)), [field], `${name} ${max + 1}자`);
  }
});

test("상한을 재는 것은 앞뒤 공백을 뗀 글이다(붙여넣을 때 딸려 온 끝 줄바꿈 때문에 거절하지 않는다)", () => {
  for (const [name, max, patch] of LIMITS) {
    assert.deepEqual(validateDraft(draft(patch("a".repeat(max) + "\n")), ENVS), [], `${name}: 끝 줄바꿈`);
    assert.deepEqual(validateDraft(draft(patch("  " + "a".repeat(max) + "  ")), ENVS), [], `${name}: 앞뒤 공백`);
  }
  // 요청에 실리는 글도 뗀 글이어서, 검사한 길이와 보내는 길이가 같다
  const r = buildStart(draft({ units: [unit({ goal: "g".repeat(MAX_GOAL) + "\n\n", check: "c".repeat(MAX_CHECK) + " " })] }), ENVS, at);
  assert.equal(r.ok, true);
  assert.equal(r.request.queue[0].prompt.length, MAX_GOAL);
  assert.equal(r.request.queue[0].check.length, MAX_CHECK);
});

test("공백뿐인 칸은 빈 칸으로 본다(탭·줄바꿈·전각 공백 포함)", () => {
  const blanks = [" ", "\t", "\n", " \r\n\t ", "\u3000", "\u00a0"];
  for (const b of blanks) {
    const got = fields(validateDraft(draft({ projectDir: b, units: [unit({ title: b, goal: b, check: b })] }), ENVS));
    assert.deepEqual(got, ["projectDir", "unit.1.title", "unit.1.goal", "unit.1.check"], JSON.stringify(b));
  }
  // 실행 이름은 비워도 된다(폴더 이름을 쓴다) — 공백뿐인 이름도 같다
  for (const b of blanks) assert.deepEqual(validateDraft(draft({ title: b }), ENVS), [], JSON.stringify(b));
});

// [프로젝트 폴더, 실행 id 끝 조각]
const RUN_ID_TAILS = [
  ["C:\\work\\toy", "toy"],
  ["C:\\work\\한글", "run"],
  ["C:\\work\\프로젝트 v2", "v2"],
  ["C:\\work\\한글-app", "app"],
  ["C:\\work\\my app (copy)", "my-app-copy"],
  ["C:\\work\\.hidden", "hidden"],
  ["C:\\work\\name.", "name"],
  ["C:\\work\\a..b", "a..b"],
  ["C:\\work\\..", "run"],
  ["C:\\work\\.", "run"],
  ["C:\\", "C"],
  ["\\\\server\\share", "share"],
  ["\\\\server\\share\\", "share"],
  ["C:\\work\\CON", "CON"],
  ["C:\\work\\a:b*c?d", "a-b-c-d"],
  ["C:\\work\\\u{1F600}", "run"],
  ["", "run"],
  ["   ", "run"],
  ["/", "run"],
  ["////", "run"],
  ["C:\\work\\ spaced \\", "spaced"],
  ["C:\\work\\" + "a".repeat(39) + "-" + "b".repeat(30), "a".repeat(39)],
];

test("실행 id: 한글·특수문자 폴더 이름은 영문 조각만 남기고, 규칙 글자 밖은 만들지 않는다", () => {
  for (const [dir, tail] of RUN_ID_TAILS) {
    const id = makeRunId(at, dir);
    assert.equal(id, `20261106-090503-${tail}`, JSON.stringify(dir));
    assert.match(id, /^[A-Za-z0-9._-]{1,64}$/, dir);
    assert.doesNotMatch(id, /[.-]$/, `${dir}: 끝이 점이나 붙임표면 윈도우가 폴더 이름을 바꾼다`);
  }
});

test("실행 id: 시각 칸은 한 자리 수를 0 으로 채우고 하루의 처음과 끝, 해의 끝을 가린다", () => {
  assert.equal(makeRunId(new Date(2026, 0, 2, 3, 4, 5), "C:\\p"), "20260102-030405-p");
  assert.equal(makeRunId(new Date(2026, 11, 31, 23, 59, 59), "C:\\p"), "20261231-235959-p");
  assert.equal(makeRunId(new Date(2026, 0, 1, 0, 0, 0), "C:\\p"), "20260101-000000-p");
});

test("실행 id: 아주 긴 폴더 이름과 이모지 이름도 규칙 길이를 넘지 않는다", () => {
  for (const name of ["x".repeat(500), "가".repeat(500), "\u{1F600}".repeat(100), "a-".repeat(300), "a.".repeat(300)]) {
    const id = makeRunId(at, `C:\\${name}`);
    assert.match(id, /^[A-Za-z0-9._-]{1,64}$/, name.slice(0, 10));
    assert.ok(id.length <= 56, `${id.length}`);
  }
});

test("폴더 이름 가장자리", () => {
  assert.equal(folderName("  C:\\a\\b  "), "b");
  assert.equal(folderName("C:\\a\\b \\"), "b");
  assert.equal(folderName("C:"), "C:");
  assert.equal(folderName("\\"), "");
  assert.equal(folderName("\\\\"), "");
  assert.equal(folderName("한글"), "한글");
  assert.equal(folderName("C:\\a/b\\c/d"), "d");
  assert.equal(folderName("relative"), "relative");
});

// [입력, 기대]
const CAPS = [
  ["", null], [" ", null], ["\t", null], ["\n", null],
  ["1", 1], ["0.5", 0.5], ["007", 7], ["00.5", 0.5], ["1.0", 1], ["  5  ", 5], ["5\n", 5], ["120", 120],
  ["0.0000001", 1e-7],
  ["1" + "0".repeat(20), 1e20],
  ["0", "bad"], ["0.0", "bad"], ["00", "bad"], ["-1", "bad"], ["-0", "bad"], ["+5", "bad"],
  ["NaN", "bad"], ["nan", "bad"], ["Infinity", "bad"], ["-Infinity", "bad"], ["1e3", "bad"], ["1E3", "bad"],
  ["0x10", "bad"], ["1_000", "bad"], ["1,5", "bad"], ["1 000", "bad"], ["1.", "bad"], [".5", "bad"], ["1..2", "bad"], ["1.2.3", "bad"],
  ["\uff11\uff12", "bad"], ["\u0663\u0664", "bad"], ["five", "bad"], ["5분", "bad"], ["$5", "bad"], ["5%", "bad"],
  ["9".repeat(400), "bad"],
];

test("상한 칸 표: 0·음수·NaN·무한·지수·소수점 가장자리·전각/아랍 숫자·아주 큰 수", () => {
  for (const [text, want] of CAPS) assert.equal(parseCap(text), want, JSON.stringify(text));
});

test("상한은 JSON 으로 보내도 그대로다(NaN·Infinity 가 null 로 바뀌어 요청에 숨지 않는다)", () => {
  for (const [text, want] of CAPS) {
    if (typeof want !== "number") continue;
    const built = buildUnit(unit({ minutes: text, usd: text }), 1, "C:\\p");
    const back = JSON.parse(JSON.stringify(built));
    assert.deepEqual(back.caps, { minutes: want, usd: want }, JSON.stringify(text));
  }
});

test("상한 칸이 어긋나면 어느 칸인지 가리고, 좋은 칸은 가리지 않는다", () => {
  const got = validateDraft(draft({ units: [unit({ minutes: "5", usd: "x" }), unit({ minutes: "-1", usd: "2" })] }), ENVS);
  assert.deepEqual(fields(got), ["unit.1.usd", "unit.2.minutes"]);
  assert.match(got[0].message, /^단위 1:.*USD/);
  assert.match(got[1].message, /^단위 2:.*분/);
});

// [종류, 샌드박스 가능?, 동의 표시, 막히는가, --accept-unsandboxed 가 붙는가]
const CONSENT_TABLE = [
  ["wsl", true, true, false, false],
  ["wsl", true, false, false, false],
  ["pwsh", false, true, false, true],
  ["pwsh", false, false, true, false],
  ["powershell", false, true, false, true],
  ["powershell", false, false, true, false],
  ["cmd", false, true, false, true],
  ["cmd", false, false, true, false],
  ["git-bash", false, true, false, true],
  ["git-bash", false, false, true, false],
];

test("동의는 샌드박스를 못 거는 환경에서만 받고, 붙이는 표시도 그때만이다", () => {
  for (const [kind, capable, consent, blocked, flag] of CONSENT_TABLE) {
    const e = { key: kind === "wsl" ? "wsl:D" : kind, label: kind, kind, distro: kind === "wsl" ? "D" : null, sandboxCapable: capable };
    const d = draft({ envKey: e.key, consent });
    assert.deepEqual(fields(validateDraft(d, [e])), blocked ? ["consent"] : [], `${kind} consent=${consent}`);
    const r = buildStart(d, [e], at);
    assert.equal(r.ok, !blocked, `${kind} consent=${consent}`);
    if (r.ok) assert.equal(r.request.acceptUnsandboxed, flag, `${kind} consent=${consent}`);
  }
});

test("동의 여부는 종류 이름이 아니라 sandboxCapable 이 가른다", () => {
  // 종류가 wsl 이라고 우기는 항목도 샌드박스를 못 건다고 하면 동의가 필요하다
  const liar = { key: "wsl:X", label: "x", kind: "wsl", distro: "X", sandboxCapable: false };
  assert.deepEqual(fields(validateDraft(draft({ envKey: "wsl:X", consent: false }), [liar])), ["consent"]);
  assert.equal(buildStart(draft({ envKey: "wsl:X", consent: true }), [liar], at).request.acceptUnsandboxed, true);
});

test("환경 목록이 비어 있으면 어떤 키를 골라도 '목록에 없다' 이고 요청은 만들지 않는다", () => {
  const r = buildStart(draft({ envKey: "pwsh" }), [], at);
  assert.equal(r.ok, false);
  assert.match(problemFor(r.problems, "env"), /목록에 없습니다/);
});

// [모델 이름, 통과?]
const MODELS = [
  ["claude-sonnet-5-5", true], ["claude-opus-4-1", true], ["sonnet", true], ["opus", true], ["haiku", true],
  ["sonnet[1m]", true], ["opus[1m]", true], ["claude_3.5", true], ["a", true], ["0", true], ["1.5-pro", true],
  [" claude-sonnet-5-5 ", true], ["opus\n", true],
  ["", false], ["  ", false], ["Sonnet", false], ["CLAUDE", false], ["-x", false], [".x", false], ["_x", false], ["[1m]", false],
  ["claude opus", false], ["x/y", false], ["x\\y", false], ["x;y", false], ["x&y", false], ["x$y", false], ["x`y", false],
  ["x\ny", false], ["x\u0000y", false], ["한글", false], ["\uff43\uff4c", false], ["sonnet\u{1F600}", false], ["--model", false],
  ["a'b", false], ['a"b', false], ["a(b)", false], ["a*", false], ["a=b", false], ["a:b", false], ["a,b", false],
];

test("모델 이름 꼴 표(백엔드 valid_model 과 같은 규칙)", () => {
  for (const [name, ok] of MODELS) {
    assert.equal(fields(validateDraft(draft({ model: name }), ENVS)).includes("model"), !ok, JSON.stringify(name));
    if (ok) assert.match(name.trim(), MODEL_PATTERN);
  }
  // 요청에는 뗀 이름이 실린다
  assert.equal(buildStart(draft({ model: "  opus \n" }), ENVS, at).request.run.model, "opus");
});

test("같은 제목의 단위가 여럿이어도 거절하지 않고 번호로 가른다", () => {
  const r = buildStart(draft({ units: [unit({ title: "같은 제목" }), unit({ title: "같은 제목" }), unit({ title: " 같은 제목 " })] }), ENVS, at);
  assert.equal(r.ok, true);
  assert.deepEqual(r.request.queue.map((q) => [q.id, q.title]), [["u1", "같은 제목"], ["u2", "같은 제목"], ["u3", "같은 제목"]]);
});

test("단위 작업 폴더: 공백뿐이면 프로젝트 폴더, 그 밖 경로는 쓴 대로 간다(검사는 명령줄 몫)", () => {
  assert.equal(buildUnit(unit({ cwd: " \t " }), 1, "  C:\\p  ").cwd, "C:\\p");
  assert.equal(buildUnit(unit({ cwd: "D:\\elsewhere" }), 1, "C:\\p").cwd, "D:\\elsewhere");
  assert.equal(buildUnit(unit({ cwd: "..\\up" }), 1, "C:\\p").cwd, "..\\up");
});

test("출발 요청 만들기는 칸(draft)을 바꾸지 않는다", () => {
  const d = draft({ title: " 이름 ", model: " opus ", projectDir: " C:\\work\\toy ", units: [unit({ title: " t ", goal: " g ", cwd: " ", minutes: " 5 " })] });
  const before = JSON.stringify(d);
  buildStart(d, ENVS, at);
  assert.equal(JSON.stringify(d), before);
});

test("출발 요청은 JSON 으로 보내도 그대로다(undefined·NaN 이 숨지 않는다)", () => {
  const r = buildStart(draft({ units: [unit({ minutes: "5", usd: "0.25" }), unit()] }), ENVS, at);
  assert.equal(r.ok, true);
  assert.deepEqual(JSON.parse(JSON.stringify(r.request)), r.request);
  assert.equal(typeof r.request.acceptUnsandboxed, "boolean");
});

test("프로젝트 폴더는 앞뒤 공백을 떼어 싣고, 끝 구분자는 그대로 둔다", () => {
  const r = buildStart(draft({ projectDir: "  C:\\work\\toy\\  ", title: "" }), ENVS, at);
  assert.equal(r.request.run.project, "toy");
  assert.equal(r.request.run.project_root, "C:\\work\\toy\\");
  assert.equal(r.request.run.title, "toy");
  assert.equal(r.request.queue[0].cwd, "C:\\work\\toy\\");
});

test("실행 이름 폴백은 영문 조각이 아니라 폴더 이름 그대로다(한글 폴더도 제목은 한글)", () => {
  const r = buildStart(draft({ projectDir: "C:\\work\\한글 앱", title: "" }), ENVS, at);
  assert.equal(r.request.run.title, "한글 앱");
  assert.equal(r.request.run.project, "한글 앱");
  assert.match(r.request.runId, /-run$/);
});

test("문제가 없는 칸은 문제 글을 돌려주지 않는다", () => {
  assert.equal(problemFor([], "model"), null);
  assert.equal(problemFor(validateDraft(draft(), ENVS), "projectDir"), null);
});

test("문제 목록 순서는 칸 순서로 일정하다(화면이 첫 문제로 초점을 옮긴다)", () => {
  const got = fields(validateDraft(draft({ projectDir: "", title: "x".repeat(MAX_TITLE + 1), envKey: null, model: "", units: [unit({ title: "" }), unit({ goal: "" })] }), ENVS));
  assert.deepEqual(got, ["projectDir", "title", "env", "model", "unit.1.title", "unit.2.goal"]);
});
