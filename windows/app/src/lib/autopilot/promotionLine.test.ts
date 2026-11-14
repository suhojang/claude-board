// @ts-nocheck — node:test 로만 돌고 svelte-check 대상이 아니다
// 자율주행 시작 확인의 승격 차례 줄(promotionLine.ts) 시험. 글은 Sources/ClaudeBoard/Autopilot.swift·Sources/AutopilotRules/Promotion.swift 에서
// 스크립트로 옮겨 그대로 견줬다(아래 SWIFT_FRAGMENTS 가 그 글 조각이다).
// 실행: cd windows/app && node --test src/lib/autopilot/promotionLine.test.ts
import assert from "node:assert/strict";
import { test } from "node:test";

import {
  addStep,
  branchRoles,
  circled,
  defaultPromotion,
  fromLabel,
  loadPromotion,
  promotionBranches,
  promotionChoices,
  promotionLine,
  promotionProblems,
  refillSteps,
  removeStep,
  rowMarker,
  setStep,
  stepSentence,
} from "./promotionLine.ts";

// Swift 소스에 그대로 있는 글 조각(스크립트가 소스에서 찾아 확인한다)
export const SWIFT_FRAGMENTS = [
  "승격 차례",
  "스토리마다 ",
  "승격할 가지(dev·live·master)가 없습니다",
  "스토리 가지",
  "autopilot 가지",
  "바꾼 차례입니다. 출발하면 프로젝트 설정에 덮어씁니다(같은 프로젝트의 끝난 실행 합치기·승격도 이 차례를 따릅니다).",
  "프로젝트 설정의 차례입니다.",
  "저장소 가지로 채웠습니다. 출발하면 프로젝트 설정에 남깁니다.",
  "바꾸기 닫기",
  "프로젝트 설정 바꾸기",
  "저장소 가지를 읽은 뒤에 바꿀 수 있습니다",
  "승격 차례를 이 자리에서 바꿉니다",
  "받을 수 있는 가지: ",
  "(묶음의 모든 저장소에 있는 가지)",
  "바꾼 승격 차례에 고칠 것이",
  "개 있습니다",
  "받는 가지",
  "가 저장소에 없습니다",
  "끼리는 올리지 않습니다",
  "올릴 가지",
  "는 앞 단계가 받는 가지가 아닙니다(자율주행 가지를 먼저 그 가지에 합치는 단계를 앞에 둡니다)",
  "뒤 조건이 받는 가지 자신(",
  "뒤라는 조건의 단계가 앞에 없습니다",
  "같은 단계(",
  "자율주행 가지",
  ")가 두 번 있습니다",
  "①②③④⑤⑥⑦⑧⑨⑩",
];

const DLM = ["dev", "live", "master"];
const step = (from, into, after = null) => ({ from, into, after });
const base = (patch = {}) => ({
  steps: defaultPromotion(DLM),
  repoBranches: [DLM],
  jira: false,
  edited: false,
  saved: true,
  starting: false,
  editorOpen: false,
  ...patch,
});

// 글 조각이 시험 파일에 그대로 있다(스크립트가 같은 조각을 Swift 소스에서도 찾는다)
test("글 조각 표가 비어 있지 않다", () => assert.ok(SWIFT_FRAGMENTS.length >= 25));

// 줄 글: 상태별 표
const LINE_TABLE = [
  ["승격할 가지 없음", { steps: [] }, "승격할 가지(dev·live·master)가 없습니다"],
  ["승격할 가지 없음(Jira)", { steps: [], jira: true }, "스토리마다 승격할 가지(dev·live·master)가 없습니다"],
  ["한 자리(dev 만)", { steps: [step("autopilot", "dev")] }, "① dev ← autopilot 가지"],
  ["세 자리 기본", {}, "① dev ← autopilot 가지 · ② live ← autopilot 가지 (dev 뒤) · ③ master ← live (live 뒤)"],
  ["세 자리 기본(Jira)", { jira: true }, "스토리마다 ① dev ← 스토리 가지 · ② live ← 스토리 가지 (dev 뒤) · ③ master ← live (live 뒤)"],
  ["같은 단계 두 번(줄에는 그대로)", { steps: [step("autopilot", "dev"), step("autopilot", "dev")] }, "① dev ← autopilot 가지 · ② dev ← autopilot 가지"],
  ["없는 가지 이름은 그대로 보인다", { steps: [step("autopilot", "release")] }, "① release ← autopilot 가지"],
];
for (const [name, patch, text] of LINE_TABLE) {
  test(`줄 글: ${name}`, () => {
    const l = promotionLine(base(patch));
    assert.equal(l.text, text);
    assert.equal(l.label, "승격 차례");
    assert.equal(l.empty, (patch.steps ?? defaultPromotion(DLM)).length === 0);
  });
}

test("줄 단계 조각: 번호·올릴 것·뒤 조건", () => {
  const l = promotionLine(base());
  assert.deepEqual(
    l.segments.map((s) => [s.marker, s.into, s.from, s.after, s.text]),
    [
      ["①", "dev", "autopilot 가지", null, "① dev ← autopilot 가지"],
      ["②", "live", "autopilot 가지", "dev", "② live ← autopilot 가지 (dev 뒤)"],
      ["③", "master", "live", "live", "③ master ← live (live 뒤)"],
    ],
  );
});

test("여섯째 단계부터 번호는 `n.`", () => {
  const steps = Array.from({ length: 7 }, (_, i) => step("autopilot", `b${i}`));
  const l = promotionLine(base({ steps, repoBranches: [steps.map((s) => s.into)] }));
  assert.deepEqual(l.segments.map((s) => s.marker), ["①", "②", "③", "④", "⑤", "6.", "7."]);
  assert.equal(rowMarker(4), "⑤");
  assert.equal(rowMarker(5), "6.");
});

test("동그라미 숫자는 10 까지, 그 밖은 `n.`", () => {
  assert.equal(circled(1), "①");
  assert.equal(circled(10), "⑩");
  assert.equal(circled(11), "11.");
  assert.equal(circled(0), "0.");
});

// 안내 줄 3상태 + 색
const NOTE_TABLE = [
  ["바꾼 차례", { edited: true, saved: true }, "바꾼 차례입니다. 출발하면 프로젝트 설정에 덮어씁니다(같은 프로젝트의 끝난 실행 합치기·승격도 이 차례를 따릅니다).", "limit", "fail"],
  ["바꾼 차례(채운 것에서)", { edited: true, saved: false }, "바꾼 차례입니다. 출발하면 프로젝트 설정에 덮어씁니다(같은 프로젝트의 끝난 실행 합치기·승격도 이 차례를 따릅니다).", "limit", "fail"],
  ["설정에 있던 차례", { edited: false, saved: true }, "프로젝트 설정의 차례입니다.", "neutral", "limit"],
  ["저장소 가지로 채움", { edited: false, saved: false }, "저장소 가지로 채웠습니다. 출발하면 프로젝트 설정에 남깁니다.", "neutral", "limit"],
];
for (const [name, patch, note, tone, ptone] of NOTE_TABLE) {
  test(`안내: ${name}`, () => {
    const l = promotionLine(base(patch));
    assert.equal(l.note, note);
    assert.equal(l.noteTone, tone);
    assert.equal(l.problemTone, ptone);
    assert.equal(l.canUndo, patch.edited);
  });
}

// 단추 상태
const BUTTON_TABLE = [
  ["저장소 읽음·닫힘", {}, true, "프로젝트 설정 바꾸기", "승격 차례를 이 자리에서 바꿉니다"],
  ["저장소 읽음·열림", { editorOpen: true }, true, "바꾸기 닫기", "승격 차례를 이 자리에서 바꿉니다"],
  ["출발 중", { starting: true }, false, "프로젝트 설정 바꾸기", "승격 차례를 이 자리에서 바꿉니다"],
  ["저장소 아직 못 읽음", { repoBranches: [] }, false, "프로젝트 설정 바꾸기", "저장소 가지를 읽은 뒤에 바꿀 수 있습니다"],
];
for (const [name, patch, editable, title, help] of BUTTON_TABLE) {
  test(`바꾸기 단추: ${name}`, () => {
    const l = promotionLine(base(patch));
    assert.equal(l.editable, editable);
    assert.equal(l.editButtonTitle, title);
    assert.equal(l.editButtonHelp, help);
  });
}

// 고칠 것
test("고칠 것 없음: 기본 차례", () => {
  const l = promotionLine(base());
  assert.deepEqual(l.problems, []);
  assert.equal(l.locksStart, false);
  assert.equal(l.startBlock, null);
});

test("고칠 것이 있어도 바꾸지 않았으면 출발을 잠그지 않는다", () => {
  const l = promotionLine(base({ steps: [step("autopilot", "release")], edited: false }));
  assert.deepEqual(l.problems, ["단계 1: 받는 가지 release 가 저장소에 없습니다"]);
  assert.equal(l.locksStart, false);
  assert.equal(l.startBlock, null);
});

test("바꾼 차례에 고칠 것이 있으면 출발을 잠그고 개수를 쓴다", () => {
  const l = promotionLine(base({ steps: [step("autopilot", "release"), step("autopilot", "release")], edited: true }));
  assert.deepEqual(l.problems, ["단계 1: 받는 가지 release 가 저장소에 없습니다", "단계 2: 받는 가지 release 가 저장소에 없습니다", "단계 2: 같은 단계(자율주행 가지 → release)가 두 번 있습니다"]);
  assert.equal(l.locksStart, true);
  assert.equal(l.startBlock, "바꾼 승격 차례에 고칠 것이 3개 있습니다");
});

const PROBLEM_TABLE = [
  ["받는 가지 없음", [step("autopilot", "x")], DLM, ["단계 1: 받는 가지 x 가 저장소에 없습니다"]],
  ["같은 가지끼리", [step("autopilot", "dev"), step("dev", "dev")], DLM, ["단계 2: 같은 가지(dev)끼리는 올리지 않습니다"]],
  ["올릴 가지 없음", [step("hotfix", "dev")], DLM, ["단계 1: 올릴 가지 hotfix 가 저장소에 없습니다"]],
  ["앞 단계가 받지 않은 가지", [step("live", "master")], DLM, ["단계 1: live 는 앞 단계가 받는 가지가 아닙니다(자율주행 가지를 먼저 그 가지에 합치는 단계를 앞에 둡니다)"]],
  ["뒤 조건이 자신", [step("autopilot", "dev", "dev")], DLM, ["단계 1: 뒤 조건이 받는 가지 자신(dev)입니다"]],
  ["뒤 조건 단계 없음", [step("autopilot", "dev", "live")], DLM, ["단계 1: live 뒤라는 조건의 단계가 앞에 없습니다"]],
  ["같은 단계 두 번(자율주행)", [step("autopilot", "dev"), step("autopilot", "dev")], DLM, ["단계 2: 같은 단계(자율주행 가지 → dev)가 두 번 있습니다"]],
  ["같은 단계 두 번(기존 가지)", [step("autopilot", "dev"), step("dev", "live"), step("dev", "live")], DLM, ["단계 3: 같은 단계(dev → live)가 두 번 있습니다"]],
];
for (const [name, steps, branches, expected] of PROBLEM_TABLE) {
  test(`고칠 것: ${name}`, () => {
    const got = promotionProblems(steps, branches);
    assert.deepEqual(got.slice(0, expected.length), expected);
  });
}

// 경계
test("빈 차례: 줄 글·고칠 것·단추", () => {
  const l = promotionLine(base({ steps: [], edited: true }));
  assert.equal(l.empty, true);
  assert.deepEqual(l.segments, []);
  assert.deepEqual(l.problems, []);
  assert.equal(l.locksStart, false);
});

test("가지가 없는 저장소: 쓸 가지 없음, 안내 글", () => {
  const l = promotionLine(base({ repoBranches: [[]], steps: [] }));
  assert.equal(l.canAdd, false);
  assert.equal(l.branchesNote, "받을 수 있는 가지: 없음");
  assert.equal(l.editable, true);
});

test("받을 수 있는 가지 안내: 한 저장소와 묶음", () => {
  assert.equal(promotionLine(base()).branchesNote, "받을 수 있는 가지: dev · live · master");
  const l = promotionLine(base({ repoBranches: [DLM, ["dev", "master", "x"]] }));
  assert.equal(l.branchesNote, "받을 수 있는 가지: dev · master(묶음의 모든 저장소에 있는 가지)");
  assert.deepEqual(l.branches, ["dev", "master"]);
});

test("묶음은 한 저장소에 없는 가지를 받는 가지로 못 쓴다", () => {
  const l = promotionLine(base({ repoBranches: [DLM, ["dev", "master"]], edited: true }));
  assert.deepEqual(l.problems, ["단계 2: 받는 가지 live 가 저장소에 없습니다", "단계 3: 올릴 가지 live 가 저장소에 없습니다"]);
});

test("autopilot/ 로 시작하는 가지는 쓸 수 있는 가지에서 빠진다", () => {
  assert.deepEqual(promotionBranches([["dev", "autopilot/run-1", "autopilot"]]), ["dev", "autopilot"]);
  assert.deepEqual(promotionBranches([]), []);
});

test("한 자리", () => {
  assert.deepEqual(defaultPromotion(["main"]), [step("autopilot", "main")]);
  assert.deepEqual(defaultPromotion(["dev"]), [step("autopilot", "dev")]);
});

test("live 가 없으면 master 는 자율주행 가지에서 dev 뒤", () => {
  assert.deepEqual(defaultPromotion(["develop", "main"]), [step("autopilot", "develop"), step("autopilot", "main", "develop")]);
});

test("가지 자리 이름: 앞 이름이 먼저, 대소문자는 다른 이름", () => {
  assert.deepEqual(branchRoles(["develop", "dev", "staging", "main", "master"]), { dev: "dev", live: "staging", master: "master" });
  assert.deepEqual(branchRoles(["Dev", "Main"]), { dev: null, live: null, master: null });
  assert.deepEqual(defaultPromotion([]), []);
});

test("없는 자리 이름(release)과 알려진 이름이 섞이면 알려진 것만 채운다", () => {
  assert.deepEqual(defaultPromotion(["release", "live"]), [step("autopilot", "live")]);
});

test("올릴 것 글: 자율주행 가지만 풀어 쓴다", () => {
  assert.equal(fromLabel("autopilot", false), "autopilot 가지");
  assert.equal(fromLabel("autopilot", true), "스토리 가지");
  assert.equal(fromLabel("Autopilot", false), "Autopilot");
  assert.equal(stepSentence(step("dev", "live", "dev"), true), "live ← dev (dev 뒤)");
});

test("loadPromotion: 설정의 차례를 읽는다", () => {
  const raw = [{ from: "autopilot", into: "dev" }, { from: "dev", into: "live", after: "dev" }, { from: 1, into: "x" }, null, "x", { into: "y" }];
  const r = loadPromotion(raw, [DLM]);
  assert.equal(r.saved, true);
  assert.deepEqual(r.steps, [step("autopilot", "dev"), step("dev", "live", "dev")]);
});

test("loadPromotion: 설정에 없거나 비면 첫 저장소 가지로 채운다", () => {
  for (const raw of [undefined, null, [], {}, "x"]) {
    const r = loadPromotion(raw, [DLM, ["main"]]);
    assert.equal(r.saved, false);
    assert.deepEqual(r.steps, defaultPromotion(DLM));
  }
  assert.deepEqual(loadPromotion([], []).steps, []);
});

test("loadPromotion: 목록은 있는데 쓸 수 있는 단계가 없어도 설정의 차례다", () => {
  assert.deepEqual(loadPromotion([{ from: 1 }], [DLM]), { steps: [], saved: true });
});

test("after 가 글이 아니면 없음", () => {
  assert.equal(loadPromotion([{ from: "autopilot", into: "dev", after: 3 }], []).steps[0].after, null);
});

// 고를 것
test("고를 수 있는 것: 둘째 단계", () => {
  const steps = defaultPromotion(DLM);
  assert.deepEqual(promotionChoices(steps, 1, DLM), { into: DLM, from: ["autopilot", "dev"], after: ["dev"] });
});

test("고를 수 있는 것: 올릴 가지는 받는 가지 후보에서 빠진다", () => {
  const steps = defaultPromotion(DLM);
  assert.deepEqual(promotionChoices(steps, 2, DLM), { into: ["dev", "master"], from: ["autopilot", "dev", "live"], after: ["dev", "live"] });
});

test("고를 수 있는 것: 새 단계와 범위 밖 번호", () => {
  const steps = defaultPromotion(DLM);
  assert.deepEqual(promotionChoices(steps, 3, DLM), { into: DLM, from: ["autopilot", "dev", "live", "master"], after: ["dev", "live", "master"] });
  assert.deepEqual(promotionChoices(steps, -1, DLM), { into: DLM, from: ["autopilot"], after: [] });
  assert.deepEqual(promotionChoices([], 0, []), { into: [], from: ["autopilot"], after: [] });
});

// 바꾸기
test("setStep: 주지 않은 칸은 그대로, after null 은 없음, 입력은 안 바뀐다", () => {
  const steps = defaultPromotion(DLM);
  const copy = JSON.stringify(steps);
  assert.deepEqual(setStep(steps, 1, { into: "master" })[1], step("autopilot", "master", "dev"));
  assert.deepEqual(setStep(steps, 1, { after: null })[1], step("autopilot", "live", null));
  assert.deepEqual(setStep(steps, 1, { from: "dev", after: "dev" })[1], step("dev", "live", "dev"));
  assert.equal(JSON.stringify(steps), copy);
});

test("setStep: 범위 밖은 바꾸지 않는다", () => {
  const steps = defaultPromotion(DLM);
  for (const i of [-1, 3, 1.5, NaN]) assert.deepEqual(setStep(steps, i, { into: "x" }), steps);
});

test("addStep: 안 쓴 가지, 다 썼으면 첫 가지, 가지가 없으면 dev", () => {
  assert.deepEqual(addStep([], DLM), [step("autopilot", "dev", null)]);
  assert.deepEqual(addStep([step("autopilot", "dev")], DLM)[1], step("dev", "live", "dev"));
  assert.deepEqual(addStep(defaultPromotion(DLM), DLM)[3], step("master", "dev", "master"));
  assert.deepEqual(addStep([], [])[0], step("autopilot", "dev", null));
});

test("removeStep / refillSteps", () => {
  const steps = defaultPromotion(DLM);
  assert.deepEqual(removeStep(steps, 0), [steps[1], steps[2]]);
  assert.deepEqual(removeStep(steps, 5), steps);
  assert.deepEqual(refillSteps(["main"]), [step("autopilot", "main")]);
});

// 긴 이름과 Windows 글
test("아주 긴 가지 이름은 줄이지 않고 그대로 보인다", () => {
  const long = "feature/" + "가".repeat(300);
  const l = promotionLine(base({ steps: [step("autopilot", long)], repoBranches: [[long]] }));
  assert.equal(l.text, `① ${long} ← autopilot 가지`);
  assert.deepEqual(l.problems, []);
  assert.equal(l.branchesNote, `받을 수 있는 가지: ${long}`);
});

test("가지 이름에 `\\`·드라이브 글자가 있어도 그대로 보인다(글 가공 없음)", () => {
  const odd = "C:\\work\\dev";
  const l = promotionLine(base({ steps: [step("autopilot", odd), step(odd, "live", odd)], repoBranches: [[odd, "live"]] }));
  assert.equal(l.text, `① ${odd} ← autopilot 가지 · ② live ← ${odd} (${odd} 뒤)`);
  assert.deepEqual(l.problems, []);
  assert.deepEqual(l.branches, [odd, "live"]);
  const p = promotionProblems([step("autopilot", "D:\\x\\y")], ["dev"]);
  assert.deepEqual(p, ["단계 1: 받는 가지 D:\\x\\y 가 저장소에 없습니다"]);
});

test("대소문자·끝 공백이 다르면 다른 가지다", () => {
  assert.deepEqual(promotionProblems([step("autopilot", "Dev")], ["dev"]), ["단계 1: 받는 가지 Dev 가 저장소에 없습니다"]);
  assert.deepEqual(promotionProblems([step("autopilot", "dev ")], ["dev"]), ["단계 1: 받는 가지 dev  가 저장소에 없습니다"]);
});

test("저장소에 없는 가지(설정의 차례가 오래된 경우)는 고칠 것으로 나온다", () => {
  const l = promotionLine(base({ steps: defaultPromotion(DLM), repoBranches: [["main"]], edited: true }));
  assert.equal(l.problems.length, 4);
  assert.equal(l.locksStart, true);
  assert.equal(l.startBlock, "바꾼 승격 차례에 고칠 것이 4개 있습니다");
});

test("줄은 입력을 고치지 않는다", () => {
  const input = base({ steps: defaultPromotion(DLM), repoBranches: [[...DLM]] });
  const before = JSON.stringify(input);
  promotionLine(input);
  assert.equal(JSON.stringify(input), before);
});
