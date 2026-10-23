// @ts-nocheck — node:test 로만 돌고 svelte-check 대상이 아니다
// 실행: cd windows/app && node --test src/lib/terminal/resizeRules.test.ts

import test from "node:test";
import assert from "node:assert/strict";

import { planSteps } from "./resizeRules.ts";

// 게이트 결정 표의 9행을 하나씩. 이름과 주석의 실측 수는 D1 게이트(부하당 캡처 5개)의 "깨진 캡처 수/5" 다.
// 각 케이스가 빨개지는 시나리오: 표의 어느 행이든 바꾸면 그 행의 실측 셀에서 깨진 캡처가 0 이 아니게 된다.

test("같은 크기면 단계가 없다", () => {
  assert.deepEqual(planSteps({ cols: 80, rows: 24 }, { cols: 80, rows: 24 }, false), []);
});

test("같은 크기라도 클램프 뒤 같으면 단계가 없다 — 80x1 → (80, -3)", () => {
  // 목표 행 -3 은 못박혀 1 이 된다. 지금 (80,1) 이므로 바꿀 것이 없다.
  assert.deepEqual(planSteps({ cols: 80, rows: 1 }, { cols: 80, rows: -3 }, false), []);
});

test("대체 화면(alt) — 어떤 방향이든 한 번, A: 100x30 → 60x20", () => {
  // 실측: tui 는 모든 모양·모든 컷에서 0/5. ConPTY 는 대체 화면에서 ESC[?25h 를 늘 내지 않는다(70/70).
  assert.deepEqual(planSteps({ cols: 100, rows: 30 }, { cols: 60, rows: 20 }, true), [
    { to: { cols: 60, rows: 20 }, cut: "A" },
  ]);
});

test("대체 화면(alt) — 늘임에서도 한 번, A: 60x20 → 100x30", () => {
  assert.deepEqual(planSteps({ cols: 60, rows: 20 }, { cols: 100, rows: 30 }, true), [
    { to: { cols: 100, rows: 30 }, cut: "A" },
  ]);
});

test("열만 줄임 — 한 번, A: 120x30 → 80x30 (게이트 실측 A 0/5)", () => {
  // 실측: A 0/5(다섯 부하 모두), B 5/5(155행), C ascii 2/5, D(오늘 방식) ascii 4/5.
  assert.deepEqual(planSteps({ cols: 120, rows: 30 }, { cols: 80, rows: 30 }, false), [
    { to: { cols: 80, rows: 30 }, cut: "A" },
  ]);
});

test("열만 줄임 — 넓은 글자가 새 폭에 걸쳐도 A: 120x30 → 79x30 (실측 ko·ko-odd 0/5)", () => {
  assert.deepEqual(planSteps({ cols: 120, rows: 30 }, { cols: 79, rows: 30 }, false), [
    { to: { cols: 79, rows: 30 }, cut: "A" },
  ]);
});

test("열만 늘임 — 한 번, B: 80x30 → 120x30 (실측 B 0/5, A 5/5(80))", () => {
  assert.deepEqual(planSteps({ cols: 80, rows: 30 }, { cols: 120, rows: 30 }, false), [
    { to: { cols: 120, rows: 30 }, cut: "B" },
  ]);
});

test("행만 늘림 — 한 번, A: 120x30 → 120x40 (실측 A ascii 0·tui 0·ink 0)", () => {
  assert.deepEqual(planSteps({ cols: 120, rows: 30 }, { cols: 120, rows: 40 }, false), [
    { to: { cols: 120, rows: 40 }, cut: "A" },
  ]);
});

test("행만 줄임 — 한 번, A(최선 노력): 120x40 → 120x30 (실측 A ascii 3/5, B·C 5/5)", () => {
  assert.deepEqual(planSteps({ cols: 120, rows: 40 }, { cols: 120, rows: 30 }, false), [
    { to: { cols: 120, rows: 30 }, cut: "A" },
  ]);
});

test("열 줄고 행 늘어 — 두 단계 열 먼저 AA: 120x30 → 60x40 (실측 0/5 다섯 부하 모두)", () => {
  assert.deepEqual(planSteps({ cols: 120, rows: 30 }, { cols: 60, rows: 40 }, false), [
    { to: { cols: 60, rows: 30 }, cut: "A" },
    { to: { cols: 60, rows: 40 }, cut: "A" },
  ]);
});

test("열 줄고 행 줄어 — 한 번, A(최선 노력): 120x40 → 60x30 (실측 A 5/5(50)·tui 0·ink 0)", () => {
  assert.deepEqual(planSteps({ cols: 120, rows: 40 }, { cols: 60, rows: 30 }, false), [
    { to: { cols: 60, rows: 30 }, cut: "A" },
  ]);
});

test("열 늘고 행 늘어 — 두 단계 행 먼저 A→B: 80x30 → 120x40 (조합은 직접 실측 없음, 각 단계 실측의 조립)", () => {
  assert.deepEqual(planSteps({ cols: 80, rows: 30 }, { cols: 120, rows: 40 }, false), [
    { to: { cols: 80, rows: 40 }, cut: "A" },
    { to: { cols: 120, rows: 40 }, cut: "B" },
  ]);
});

test("열 늘고 행 줄어 — 두 단계 행 먼저 A→B(최선 노력): 80x40 → 120x30", () => {
  assert.deepEqual(planSteps({ cols: 80, rows: 40 }, { cols: 120, rows: 30 }, false), [
    { to: { cols: 80, rows: 30 }, cut: "A" },
    { to: { cols: 120, rows: 30 }, cut: "B" },
  ]);
});

test("가장 작은 열 바꾸기 1 — 줄임은 A: 80x24 → 79x24", () => {
  assert.deepEqual(planSteps({ cols: 80, rows: 24 }, { cols: 79, rows: 24 }, false), [
    { to: { cols: 79, rows: 24 }, cut: "A" },
  ]);
});

test("가장 작은 열 바꾸기 2 — 늘임은 B: 79x24 → 80x24", () => {
  assert.deepEqual(planSteps({ cols: 79, rows: 24 }, { cols: 80, rows: 24 }, false), [
    { to: { cols: 80, rows: 24 }, cut: "B" },
  ]);
});

test("가장 작은 행 바꾸기 1 — 늘림은 A: 80x23 → 80x24", () => {
  assert.deepEqual(planSteps({ cols: 80, rows: 23 }, { cols: 80, rows: 24 }, false), [
    { to: { cols: 80, rows: 24 }, cut: "A" },
  ]);
});

test("가장 작은 행 바꾸기 2 — 줄임은 A: 80x24 → 80x23", () => {
  assert.deepEqual(planSteps({ cols: 80, rows: 24 }, { cols: 80, rows: 23 }, false), [
    { to: { cols: 80, rows: 23 }, cut: "A" },
  ]);
});

test("목표는 열 2 × 행 1 아래로 못박힌다 — 5x5 → (1, 10) 은 2x10", () => {
  assert.deepEqual(planSteps({ cols: 5, rows: 5 }, { cols: 1, rows: 10 }, false), [
    { to: { cols: 2, rows: 5 }, cut: "A" },
    { to: { cols: 2, rows: 10 }, cut: "A" },
  ]);
});

test("목표는 열 2 × 행 1 아래로 못박힌다 — 5x5 → (0, 0) 은 2x1", () => {
  assert.deepEqual(planSteps({ cols: 5, rows: 5 }, { cols: 0, rows: 0 }, false), [
    { to: { cols: 2, rows: 1 }, cut: "A" },
  ]);
});

test("음수 목표 열도 못박힌다 — 30x10 → (0, 50) 은 2x50", () => {
  assert.deepEqual(planSteps({ cols: 30, rows: 10 }, { cols: 0, rows: 50 }, false), [
    { to: { cols: 2, rows: 10 }, cut: "A" },
    { to: { cols: 2, rows: 50 }, cut: "A" },
  ]);
});

test("잘못된 입력은 지금 터뜨린다 — from NaN", () => {
  assert.throws(() => planSteps({ cols: NaN, rows: 24 }, { cols: 80, rows: 24 }, false), /from\.cols/);
});

test("잘못된 입력은 지금 터뜨린다 — to 소수", () => {
  assert.throws(() => planSteps({ cols: 80, rows: 24 }, { cols: 80.5, rows: 24 }, false), /to\.cols/);
});

test("잘못된 입력은 지금 터뜨린다 — to NaN", () => {
  assert.throws(() => planSteps({ cols: 80, rows: 24 }, { cols: 80, rows: NaN }, false), /to\.rows/);
});

test("잘못된 입력은 지금 터뜨린다 — from 0x5", () => {
  assert.throws(() => planSteps({ cols: 0, rows: 5 }, { cols: 80, rows: 24 }, false), /1x1/);
});

// 계약의 사후조건 — 어떤 입력이든 마지막 단계는 늘 못박힌 목표에서 끝나고, 단계들은 사슬을 이룬다.
test("모든 모양에서 마지막 단계는 못박힌 목표에서 끝난다", () => {
  const nows = [
    { cols: 2, rows: 1 },
    { cols: 3, rows: 3 },
    { cols: 80, rows: 24 },
    { cols: 120, rows: 40 },
  ];
  const goals = [
    { cols: 60, rows: 20 },
    { cols: 120, rows: 30 },
    { cols: 100, rows: 40 },
    { cols: 0, rows: 0 },
    { cols: 1, rows: 5 },
    { cols: -3, rows: -3 },
  ];

  for (const now of nows)
    for (const goal of goals)
      for (const alt of [false, true]) {
        const steps = planSteps(now, goal, alt);
        const expected = {
          cols: Math.max(2, goal.cols),
          rows: Math.max(1, goal.rows),
        };
        if (now.cols === expected.cols && now.rows === expected.rows) {
          assert.deepEqual(steps, [], `${now.cols}x${now.rows} → 목표는 이미 도달했다`);
        } else {
          assert.ok(steps.length > 0, `${now.cols}x${now.rows} → ${goal.cols}x${goal.rows} alt=${alt}: 단계가 있어야 한다`);
          const last = steps[steps.length - 1];
          assert.deepEqual(last.to, expected, `${now.cols}x${now.rows} → ${goal.cols}x${goal.rows} alt=${alt}: 마지막 단계가 목표다`);
          for (const step of steps) {
            assert.ok(step.to.cols >= 2 && step.to.rows >= 1, "모든 단계 크기는 못박침을 지킨다");
          }
        }
      }
});

test("같은 입력에 같은 답 — 규칙은 순수하다", () => {
  const first = planSteps({ cols: 120, rows: 30 }, { cols: 60, rows: 40 }, false);
  const second = planSteps({ cols: 120, rows: 30 }, { cols: 60, rows: 40 }, false);
  assert.deepEqual(first, second);
  assert.notEqual(first, second); // 배열을 새로 만든다 — 밖의 상태를 건드리지 않았다는 증거
  first[0].to.cols = 999; // 돌려준 배열을 건드려도 다음 호출에 안 번진다
  const third = planSteps({ cols: 120, rows: 30 }, { cols: 60, rows: 40 }, false);
  assert.equal(third[0].to.cols, 60);
});