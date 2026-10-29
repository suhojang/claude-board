// @ts-nocheck — node:test 로만 돌고 svelte-check 대상이 아니다
// 세션 줄 메뉴가 내는 동작 이름이 mac performAction 어휘(+ Windows 전용 activate)뿐인지, 상태별 항목·설명이 mac 표와 같은지 시험한다.
// 실행: cd windows/app && node --test src/lib/board/sessionActions.test.ts   (Node 22.18+/24 는 .ts 를 바로 읽는다)
import assert from "node:assert/strict";
import { test } from "node:test";

import { sessionMenuEntries, type SessionMenuEntry } from "./sessionActions.ts";

const ALLOWED = ["activate", "focus", "reopen", "pause", "resume", "stop", "moveToBoard"];
const STATES = ["working", "waiting", "idle", "paused", "ended"] as const;
const TAB = { kind: "tab", surfaceId: "s1", headless: false };
const TERMINAL = { kind: "terminal" };
const BACKGROUND = { kind: "background" };
const ENDED = { kind: "ended" };

const sessionOf = (state: (typeof STATES)[number], over = {}) => ({ state, paused: false, canDrive: true, headless: false, ...over });

const actionsOf = (entries: SessionMenuEntry[]) => entries.filter((entry) => !("separator" in entry));
const names = (entries: SessionMenuEntry[]) => actionsOf(entries).map((entry) => entry.action);
const pauseEntry = (state, canDrive) => actionsOf(sessionMenuEntries(sessionOf(state, { canDrive }), TAB)).find((e) => e.action === "pause");

test("메뉴는 어느 상태·자리에서든 mac 어휘의 동작 이름만 낸다", () => {
  for (const state of STATES) {
    for (const paused of [false, true]) {
      for (const place of [TAB, TERMINAL, ENDED]) {
        for (const name of names(sessionMenuEntries(sessionOf(state, { paused }), place))) {
          assert.ok(ALLOWED.includes(name), `${state}/${paused}/${place.kind}: ${name}`);
        }
      }
    }
  }
});

test("끝난 세션은 reopen, 일시중지는 resume·stop, 그 밖은 pause·stop", () => {
  assert.deepEqual(names(sessionMenuEntries(sessionOf("ended"), ENDED)), ["reopen"]);
  assert.deepEqual(names(sessionMenuEntries(sessionOf("paused"), TAB)), ["resume", "stop"]);
  for (const state of ["working", "waiting", "idle"] as const) {
    assert.deepEqual(names(sessionMenuEntries(sessionOf(state), TAB)), ["pause", "stop"]);
    // 다른 터미널에서 키를 넣을 수 있으면 일시중지와 중단 사이에 보드로 옮기기가 낀다
    assert.deepEqual(names(sessionMenuEntries(sessionOf(state), TERMINAL)), ["pause", "moveToBoard", "stop"]);
  }
});

test("보드로 옮기기는 place=terminal 이고 canDrive 일 때만, 살아 있고 일시중지가 아닌 세션에만 낸다", () => {
  const offered = (state, canDrive, place) => names(sessionMenuEntries(sessionOf(state, { canDrive }), place)).includes("moveToBoard");
  for (const state of STATES) {
    for (const canDrive of [true, false]) {
      for (const place of [TAB, TERMINAL, BACKGROUND, ENDED]) {
        const expected = place.kind === "terminal" && canDrive && ["working", "waiting", "idle"].includes(state);
        assert.equal(offered(state, canDrive, place), expected, `${state}/${canDrive}/${place.kind}`);
      }
    }
  }
});

test("보드로 옮기기 항목의 문구는 mac 과 같고 일시중지 다음·구분선 앞에 온다", () => {
  const entries = sessionMenuEntries(sessionOf("working"), TERMINAL);
  assert.equal(entries.length, 4);
  assert.equal(entries[1].label, "보드로 옮기기");
  assert.equal(entries[1].detail, "그쪽을 끝내고 보드 새 탭에서 이어 엽니다");
  assert.equal(entries[1].enabled, undefined);
  assert.deepEqual(entries[2], { separator: true });
  assert.equal(entries[3].action, "stop");
});

test("살아 있는 background 세션은 메뉴가 없다", () => {
  for (const state of ["working", "waiting", "idle", "paused"] as const) {
    assert.deepEqual(sessionMenuEntries(sessionOf(state, { headless: true }), BACKGROUND), []);
  }
});

test("끝난 세션 — 일시중지 표시가 있으면 재개 문구와 carryOn true, 없으면 다시 열기와 carryOn false", () => {
  const [plain] = sessionMenuEntries(sessionOf("ended"), ENDED);
  const [carried] = sessionMenuEntries(sessionOf("ended", { paused: true }), ENDED);
  assert.deepEqual(
    { label: plain.label, carryOn: plain.carryOn, action: plain.action },
    { label: "세션 다시 열기", carryOn: false, action: "reopen" },
  );
  assert.deepEqual(
    { label: carried.label, carryOn: carried.carryOn, action: carried.action },
    { label: "세션 재개 (이어서 진행)", carryOn: true, action: "reopen" },
  );
  assert.equal(plain.detail, "보드 새 탭에서 이 대화를 이어 엽니다");
  assert.equal(carried.detail, "보드 새 탭에서 열고 멈춘 작업을 이어 갑니다");
});

test("끝난 headless 세션의 다시 열기는 대화형으로 이어 연다는 설명을 단다", () => {
  const [entry] = sessionMenuEntries(sessionOf("ended", { headless: true }), ENDED);
  assert.equal(entry.label, "세션 다시 열기");
  assert.equal(entry.detail, "보드 새 탭에서 대화형으로 이어 엽니다");
});

test("일시중지 세션 — 재개와 중단, 설명 포함", () => {
  const [resume, , stop] = sessionMenuEntries(sessionOf("paused"), TAB);
  assert.equal(resume.detail, "멈춘 지점부터 이어서 진행하게 합니다");
  assert.deepEqual({ label: stop.label, detail: stop.detail }, { label: "세션 중단", detail: "응답을 멈추고 끝냅니다. 대화 기록은 남습니다" });
});

test("working·waiting 에서 키를 넣을 수 있으면 일시중지가 켜진다", () => {
  for (const state of ["working", "waiting"] as const) {
    const entry = pauseEntry(state, true);
    assert.equal(entry.enabled, true);
    assert.equal(entry.detail, "지금 응답을 멈춥니다. 재개하면 이어 갑니다");
  }
});

test("idle 이면 일시중지가 꺼지고 대기 설명이 붙는다", () => {
  const entry = pauseEntry("idle", true);
  assert.equal(entry.enabled, false);
  assert.equal(entry.detail, "대기 중이라 멈출 응답이 없습니다");
});

test("키를 넣을 수 없으면 일시중지가 꺼지고 키 설명이 붙는다", () => {
  const entry = pauseEntry("working", false);
  assert.equal(entry.enabled, false);
  assert.equal(entry.detail, "이 터미널에는 키를 넣을 수 없습니다");
});

test("끝난 headless 이면서 일시중지 표시가 있으면 재개 문구(carryOn true)다", () => {
  const [entry] = sessionMenuEntries(sessionOf("ended", { headless: true, paused: true }), ENDED);
  assert.deepEqual({ label: entry.label, carryOn: entry.carryOn }, { label: "세션 재개 (이어서 진행)", carryOn: true });
});

test("끝난 세션은 자리와 canDrive 와 상관없이 같은 항목 하나", () => {
  const baseline = JSON.stringify(sessionMenuEntries(sessionOf("ended"), ENDED));
  for (const place of [TAB, TERMINAL, BACKGROUND, ENDED]) {
    for (const canDrive of [true, false]) {
      assert.equal(JSON.stringify(sessionMenuEntries(sessionOf("ended", { canDrive }), place)), baseline, place.kind);
    }
  }
});

test("끝난 세션의 메뉴는 reopen 뿐이다", () => {
  for (const paused of [true, false]) {
    for (const headless of [true, false]) {
      assert.deepEqual(names(sessionMenuEntries(sessionOf("ended", { paused, headless }), ENDED)), ["reopen"]);
    }
  }
});

test("살아 있는 background 는 canDrive 와 상관없이 메뉴가 없다", () => {
  for (const state of ["working", "waiting", "idle", "paused"] as const) {
    for (const canDrive of [true, false]) {
      assert.deepEqual(sessionMenuEntries(sessionOf(state, { canDrive, headless: false }), BACKGROUND), []);
    }
  }
});

test("idle 이고 키도 못 넣으면 대기 설명이 이긴다", () => {
  const entry = pauseEntry("idle", false);
  assert.equal(entry.enabled, false);
  assert.equal(entry.detail, "대기 중이라 멈출 응답이 없습니다");
});

test("waiting 이고 키를 못 넣으면 꺼지고 키 설명이다", () => {
  const entry = pauseEntry("waiting", false);
  assert.equal(entry.enabled, false);
  assert.equal(entry.detail, "이 터미널에는 키를 넣을 수 없습니다");
});

test("terminal 자리에서도 pause 규칙은 tab 과 같다(옮기기 항목만 더 있다)", () => {
  for (const state of ["working", "waiting", "idle"] as const) {
    for (const canDrive of [true, false]) {
      const inTab = sessionMenuEntries(sessionOf(state, { canDrive }), TAB);
      const inTerminal = sessionMenuEntries(sessionOf(state, { canDrive }), TERMINAL).filter((e) => e.action !== "moveToBoard");
      assert.equal(JSON.stringify(inTerminal), JSON.stringify(inTab));
    }
  }
});

test("terminal 자리에서 키를 못 넣으면 tab 과 메뉴가 같다", () => {
  for (const state of ["working", "waiting", "idle"] as const) {
    assert.equal(
      JSON.stringify(sessionMenuEntries(sessionOf(state, { canDrive: false }), TERMINAL)),
      JSON.stringify(sessionMenuEntries(sessionOf(state, { canDrive: false }), TAB)),
    );
  }
});

test("일시중지 세션의 재개는 canDrive 와 상관없이 꺼지지 않는다", () => {
  for (const canDrive of [true, false]) {
    const [resume] = sessionMenuEntries(sessionOf("paused", { canDrive }), TERMINAL);
    assert.equal(resume.action, "resume");
    assert.notEqual(resume.enabled, false);
  }
});

test("살아 있는 메뉴는 항목-구분선-중단 모양이고 중단이 마지막이다", () => {
  for (const state of ["working", "waiting", "idle", "paused"] as const) {
    const entries = sessionMenuEntries(sessionOf(state), TAB);
    assert.equal(entries.length, 3);
    assert.deepEqual(entries[1], { separator: true });
    assert.equal(entries[2].action, "stop");
  }
});

test("보드로 옮기기 항목도 호출마다 복사본이라 한쪽을 고쳐도 다음 메뉴가 변하지 않는다", () => {
  const a = sessionMenuEntries(sessionOf("working"), TERMINAL);
  a[1].label = "오염";
  assert.equal(sessionMenuEntries(sessionOf("working"), TERMINAL)[1].label, "보드로 옮기기");
});

// 건너뜀: sessionActions.ts:19 의 STOP_ENTRY 상수 하나를 모든 메뉴가 같은 객체로 돌려준다(복사하지 않음).
// 호출자가 항목을 고치면 다음 메뉴까지 오염된다. 중단 항목은 호출마다 복사해 돌려준다.
test("메뉴를 두 번 만들어도 한쪽을 고쳐 다른 쪽이 변하지 않는다(공유 중단 항목 오염)", () => {
  const a = sessionMenuEntries(sessionOf("working"), TAB);
  const b = sessionMenuEntries(sessionOf("working"), TAB);
  a[2].label = "오염";
  assert.equal(b[2].label, "세션 중단");
  assert.equal(sessionMenuEntries(sessionOf("idle"), TAB)[2].label, "세션 중단");
});
