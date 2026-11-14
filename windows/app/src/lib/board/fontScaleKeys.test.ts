// @ts-nocheck — node:test 로만 돌고 svelte-check 대상이 아니다
// 실행: cd windows/app && node --test src/lib/board/fontScaleKeys.test.ts
import test from "node:test";
import assert from "node:assert/strict";
import { MOD, keyOf, label, same } from "../shortcuts/keyCombo.ts";
import { SHORTCUT_ACTIONS } from "../shortcuts/actions.ts";
import { RESERVED, acceptable } from "../shortcuts/rules.ts";
import { BOARD_FONT_ACTIONS, BOARD_FONT_STEP, boardFontScaleChange, isBoardFontAction } from "./fontScaleKeys.ts";

const byId = Object.fromEntries(BOARD_FONT_ACTIONS.map((a) => [a.id, a]));

// 표 전체 = 지금 actions.ts 표 + 이 모듈의 행(아직 표에 안 넣었으면 더해서 본다. 넣은 뒤에는 같은 id 를 한 번만 센다)
const wired = new Set(SHORTCUT_ACTIONS.map((a) => a.id));
const FULL_TABLE = [...SHORTCUT_ACTIONS, ...BOARD_FONT_ACTIONS.filter((a) => !wired.has(a.id))];

// 아직 표에 없지만 결정 D6(gap-summary.md)이 이미 정한 기본 키 — 이 키들과도 겹치면 안 된다
const D6_PLANNED = {
  equalize: keyOf("Equal", MOD.ctrl | MOD.alt),
  splitRight: keyOf("KeyD", MOD.ctrl | MOD.alt),
  splitDown: keyOf("KeyD", MOD.ctrl | MOD.alt | MOD.shift),
  prevSurface: keyOf("BracketLeft", MOD.ctrl | MOD.alt),
  nextSurface: keyOf("BracketRight", MOD.ctrl | MOD.alt),
  zoom: keyOf("Enter", MOD.ctrl | MOD.alt),
  terminalBigger: keyOf("Equal", MOD.ctrl | MOD.shift),
  terminalBigger2: keyOf("Equal", MOD.ctrl),
  terminalSmaller: keyOf("Minus", MOD.ctrl),
  terminalReset: keyOf("Digit0", MOD.ctrl),
  resizeUp: keyOf("ArrowUp", MOD.ctrl | MOD.alt | MOD.shift),
  resizeDown: keyOf("ArrowDown", MOD.ctrl | MOD.alt | MOD.shift),
  resizeLeft: keyOf("ArrowLeft", MOD.ctrl | MOD.alt | MOD.shift),
  resizeRight: keyOf("ArrowRight", MOD.ctrl | MOD.alt | MOD.shift),
};

test("행 — mac 표 순서·이름, 묶음은 글자 크기", () => {
  assert.deepEqual(BOARD_FONT_ACTIONS.map((a) => a.id), ["boardBigger", "boardBigger2", "boardSmaller", "boardReset"]);
  assert.deepEqual(BOARD_FONT_ACTIONS.map((a) => a.title), ["보드 글자 크게", "보드 글자 크게 (보조)", "보드 글자 작게", "보드 글자 원래 크기"]);
  for (const a of BOARD_FONT_ACTIONS) assert.equal(a.group, "글자 크기");
});

test("기본 키 — Ctrl+Alt+0 은 보드 보기(showBoard)가 쓰므로 원래 크기는 Shift 를 더한다", () => {
  assert.equal(label(byId.boardBigger.initial), "Ctrl+Alt+Shift+=");
  assert.equal(label(byId.boardBigger2.initial), "Ctrl+Alt+Num +");
  assert.equal(label(byId.boardSmaller.initial), "Ctrl+Alt+Shift+-");
  assert.equal(label(byId.boardReset.initial), "Ctrl+Alt+Shift+0");
  assert.equal(label(SHORTCUT_ACTIONS.find((a) => a.id === "showBoard").initial), "Ctrl+Alt+0");
});

test("표 전체 — 기본 키가 있는 동작끼리 같은 키가 없다(보드 글자 넷 포함)", () => {
  const seen = new Map();
  for (const a of FULL_TABLE) {
    if (!a.initial) continue;
    const key = label(a.initial);
    assert.ok(!seen.has(key), `${key}: ${seen.get(key)} 와 ${a.id}`);
    seen.set(key, a.id);
  }
  assert.ok(seen.has("Ctrl+Alt+Shift+0"));
  assert.equal(new Set(FULL_TABLE.map((a) => a.id)).size, FULL_TABLE.length, "id 겹침");
});

test("보드 글자 키는 결정 D6 이 정한 앞으로의 기본 키(고르게·나누기·터미널 글자 …)와도 겹치지 않는다", () => {
  for (const a of BOARD_FONT_ACTIONS) {
    for (const [name, key] of Object.entries(D6_PLANNED)) assert.ok(!same(a.initial, key), `${a.id} 가 ${name} (${label(key)}) 와 같다`);
  }
});

test("보드 글자 키는 고를 수 있는 키이고 고를 수 없는 키 목록과 겹치지 않는다", () => {
  for (const a of BOARD_FONT_ACTIONS) {
    assert.equal(acceptable(a.initial), "ok", a.id);
    assert.ok(!RESERVED.some((r) => same(r, a.initial)), a.id);
  }
});

test("표에 이미 넣었다면 이 모듈의 행과 같아야 한다(드리프트 검사)", () => {
  for (const a of BOARD_FONT_ACTIONS) {
    const row = SHORTCUT_ACTIONS.find((x) => x.id === a.id);
    if (!row) continue;
    assert.deepEqual(row, a, a.id);
  }
});

test("걸음 표 — 행 넷과 일대일", () => {
  assert.deepEqual(Object.keys(BOARD_FONT_STEP).sort(), BOARD_FONT_ACTIONS.map((a) => a.id).sort());
  assert.equal(BOARD_FONT_STEP.boardBigger, 1);
  assert.equal(BOARD_FONT_STEP.boardBigger2, 1);
  assert.equal(BOARD_FONT_STEP.boardSmaller, -1);
  assert.equal(BOARD_FONT_STEP.boardReset, null);
});

test("isBoardFontAction — 넷만 true, 다른 id 와 Object 속성 이름은 false", () => {
  for (const a of BOARD_FONT_ACTIONS) assert.equal(isBoardFontAction(a.id), true);
  for (const id of ["terminalBigger", "showBoard", "tab1", "", "toString", "constructor", "__proto__"]) assert.equal(isBoardFontAction(id), false, id);
});

test("크게·작게 — 한 단계씩, 보조 키도 크게와 같다", () => {
  assert.deepEqual(boardFontScaleChange("boardBigger", 1), { scale: 1.1, toast: "보드 글자 110%", changed: true });
  assert.deepEqual(boardFontScaleChange("boardBigger2", 1.1), { scale: 1.2, toast: "보드 글자 120%", changed: true });
  assert.deepEqual(boardFontScaleChange("boardSmaller", 1), { scale: 0.9, toast: "보드 글자 90%", changed: true });
  assert.deepEqual(boardFontScaleChange("boardSmaller", 1.1), { scale: 1, toast: "보드 글자 100%", changed: true });
});

test("원래 크기 — 어느 배율에서든 100%, 이미 100% 면 changed 만 false", () => {
  assert.deepEqual(boardFontScaleChange("boardReset", 1.5), { scale: 1, toast: "보드 글자 100%", changed: true });
  assert.deepEqual(boardFontScaleChange("boardReset", 0.8), { scale: 1, toast: "보드 글자 100%", changed: true });
  assert.deepEqual(boardFontScaleChange("boardReset", 1), { scale: 1, toast: "보드 글자 100%", changed: false });
});

test("끝 — 150%·80% 에서도 글은 그대로 보이고(mac 도 toast 를 부른다) 배율은 넘지 않는다", () => {
  assert.deepEqual(boardFontScaleChange("boardBigger", 1.5), { scale: 1.5, toast: "보드 글자 150%", changed: false });
  assert.deepEqual(boardFontScaleChange("boardBigger2", 1.5), { scale: 1.5, toast: "보드 글자 150%", changed: false });
  assert.deepEqual(boardFontScaleChange("boardSmaller", 0.8), { scale: 0.8, toast: "보드 글자 80%", changed: false });
});

test("전 구간 — 80% 에서 크게를 아홉 번 눌러 150% 에 닿고, 작게를 아홉 번 눌러 80% 로 돌아온다. 부동소수점 꼬리가 없다", () => {
  let s = 0.8;
  const up = [];
  for (let i = 0; i < 9; i++) {
    const c = boardFontScaleChange("boardBigger", s);
    up.push(c.toast);
    s = c.scale;
  }
  assert.deepEqual(up, ["보드 글자 90%", "보드 글자 100%", "보드 글자 110%", "보드 글자 120%", "보드 글자 130%", "보드 글자 140%", "보드 글자 150%", "보드 글자 150%", "보드 글자 150%"]);
  assert.equal(s, 1.5);
  for (let i = 0; i < 9; i++) s = boardFontScaleChange("boardSmaller", s).scale;
  assert.equal(s, 0.8);
  assert.equal(boardFontScaleChange("boardBigger", 1.2).scale, 1.3, "1.2 + 0.1 이 1.3000000000000003 으로 새지 않는다");
});

test("설정 값이 없거나 깨졌으면 100% 로 보고 시작한다", () => {
  assert.equal(boardFontScaleChange("boardBigger", undefined).scale, 1.1);
  assert.equal(boardFontScaleChange("boardSmaller", null).scale, 0.9);
  assert.equal(boardFontScaleChange("boardBigger", "abc").scale, 1.1);
  assert.equal(boardFontScaleChange("boardBigger", NaN).scale, 1.1);
  assert.equal(boardFontScaleChange("boardBigger", 9).scale, 1.5, "범위 밖 저장 값은 끝값으로");
});

test("보드 글자 동작이 아니면 null — run.ts 는 아무것도 하지 않는다", () => {
  for (const id of ["terminalBigger", "showBoard", "tab1", "", "toString", "__proto__"]) assert.equal(boardFontScaleChange(id, 1), null, id);
});
