// @ts-nocheck — node:test 로만 돌고 svelte-check 대상이 아니다
// 실행: cd windows/app && node --test src/lib/settings/screenRules.test.ts
import { test } from "node:test";
import assert from "node:assert/strict";
import { escapeStep, collapseOnOpen, afterClose, shouldDismissForCollapse } from "./screenRules.ts";

test("Esc 차례 — 키 받기 취소 → 검색어 지우기 → 닫기", () => {
  const table = [
    [{ recording: true, query: "알림" }, "cancelRecording"],
    [{ recording: true, query: "" }, "cancelRecording"],
    [{ recording: false, query: "알림" }, "clearQuery"],
    [{ recording: false, query: "  " }, "clearQuery"], // 공백만 있어도 칸은 비워야 한다
    [{ recording: false, query: "" }, "close"],
  ];
  for (const [state, want] of table) assert.equal(escapeStep(state), want, JSON.stringify(state));
});

test("Esc 를 세 번 누르면 세 단계를 차례로 지난다", () => {
  const state = { recording: true, query: "x" };
  const seen = [];
  for (let i = 0; i < 3; i++) {
    const step = escapeStep(state);
    seen.push(step);
    if (step === "cancelRecording") state.recording = false;
    else if (step === "clearQuery") state.query = "";
  }
  assert.deepEqual(seen, ["cancelRecording", "clearQuery", "close"]);
});

test("열 때 — 접힌 보드면 잠깐 펼치고, 펼쳐 있거나 상태를 모르면 건드리지 않는다", () => {
  assert.equal(collapseOnOpen({ snapKnown: true, expanded: false }), true);
  assert.equal(collapseOnOpen({ snapKnown: true, expanded: true }), false);
  assert.equal(collapseOnOpen({ snapKnown: false, expanded: false }), false);
});

test("닫을 때 — 잠깐 펼쳤고 keep 이 아니면 다시 접는다", () => {
  assert.equal(afterClose({ collapsedByUs: true, keep: false }), "recollapse");
  assert.equal(afterClose({ collapsedByUs: true, keep: true }), "stay"); // 탭을 다루는 동작이면 펼친 채
  assert.equal(afterClose({ collapsedByUs: false, keep: false }), "stay");
  assert.equal(afterClose({ collapsedByUs: false, keep: true }), "stay");
});

test("터미널 칸이 접히면 설정 화면도 내려간다 — 상태를 알고 떠 있을 때만", () => {
  assert.equal(shouldDismissForCollapse({ shown: true, snapKnown: true, expanded: false }), true);
  assert.equal(shouldDismissForCollapse({ shown: true, snapKnown: true, expanded: true }), false);
  assert.equal(shouldDismissForCollapse({ shown: false, snapKnown: true, expanded: false }), false);
  assert.equal(shouldDismissForCollapse({ shown: true, snapKnown: false, expanded: false }), false);
});
