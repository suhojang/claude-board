// @ts-nocheck — node:test 로만 돌고 svelte-check 대상이 아니다
// 자율주행 줄의 글·짝짓기·단추 표를 시험한다.
// 실행: cd windows/app && node --test src/lib/board/autopilotView.test.ts   (Node 22.18+/24 는 .ts 를 바로 읽는다)
import assert from "node:assert/strict";
import { test } from "node:test";

import { buttons, clockText, costText, lineText, orphanRuns, runsForGroup, samePath, stateLabel, tallyText, waitText } from "./autopilotView.ts";

const run = (patch = {}) => ({
  id: "r1",
  path: "C:\\Users\\demo\\work\\shop-api",
  title: "결과 파일 맞추기",
  state: "running",
  reason: "",
  waitUntil: null,
  env: { key: "cmd", label: "명령 프롬프트", sandbox: false },
  unit: { n: 2, title: "결과 파일 맞추기", phase: "work", activeSec: 720, costUsd: 0.42, tries: 1 },
  total: 5,
  done: 1,
  failed: 0,
  capped: 1,
  skipped: 0,
  stopped: 0,
  can: ["pause", "stop"],
  ended: false,
  ...patch,
});

// 지역 시각으로 만든 ms — 시간대에 상관없이 같은 지역 날짜를 얻는다
const local = (y, mo, d, h, mi) => new Date(y, mo - 1, d, h, mi).getTime();

test("samePath_table", () => {
  const rows = [
    ["C:\\Work\\Shop", "c:/work/shop", true],
    ["C:\\Work\\Shop\\", "C:\\Work\\Shop", true],
    ["C:/Work/Shop//", "c:\\work\\shop", true],
    ["C:\\Work\\Shop", "C:\\Work\\Shop2", false],
    ["C:\\Work\\Shop", "C:\\Work", false],
    ["", "C:\\Work", false],
  ];
  for (const [a, b, expected] of rows) assert.equal(samePath(a, b), expected, `${a} / ${b}`);
});

test("runsForGroup_matches_only_same_path", () => {
  const a = run({ id: "a", path: "C:\\work\\shop" });
  const b = run({ id: "b", path: "C:\\work\\other" });
  const c = run({ id: "c", path: "c:/work/shop/" });
  assert.deepEqual(runsForGroup([a, b, c], "C:\\Work\\Shop").map((x) => x.id), ["a", "c"]);
  assert.deepEqual(runsForGroup([b], "C:\\work\\shop"), []);
});

test("orphanRuns_unmatched", () => {
  const a = run({ id: "a", path: "C:\\work\\shop" });
  const b = run({ id: "b", path: "C:\\work\\lost" });
  assert.deepEqual(orphanRuns([a, b], ["c:/work/shop", "C:\\work\\infra"]).map((x) => x.id), ["b"]);
  assert.deepEqual(orphanRuns([a, b], []).map((x) => x.id), ["a", "b"]);
});

test("lineText_running_cmd_no_sandbox", () => {
  const t = lineText(run(), local(2026, 11, 5, 12, 0));
  assert.deepEqual(t, {
    state: "도는 중",
    env: "명령 프롬프트",
    noSandbox: true,
    unit: "단위 2/5: 결과 파일 맞추기",
    clock: "12분",
    cost: "이번 단위 0.420 USD",
    tally: "통과 1 · 미통과 0 · 상한 1",
    wait: null,
  });
});

test("lineText_wsl_sandbox", () => {
  const t = lineText(run({ env: { key: "wsl:Ubuntu", label: "WSL · Ubuntu", sandbox: true }, state: "waiting_limit" }), 0);
  assert.equal(t.env, "WSL · Ubuntu");
  assert.equal(t.noSandbox, false);
  assert.equal(t.state, "사용량 한도 대기");
});

test("lineText_unknown_env", () => {
  const t = lineText(run({ env: { key: "", label: "", sandbox: false } }), 0);
  assert.equal(t.env, "알 수 없음");
  assert.equal(t.noSandbox, true);
});

test("lineText_no_unit", () => {
  const t = lineText(run({ unit: null }), 0);
  assert.equal(t.unit, null);
  assert.equal(t.clock, null);
  assert.equal(t.cost, null);
});

test("clock_format_table", () => {
  const rows = [[0, "0초"], [45, "45초"], [60, "1분"], [720, "12분"], [3780, "1시간 3분"], [3600, "1시간"]];
  for (const [sec, expected] of rows) assert.equal(clockText(sec), expected, `${sec}초`);
});

test("cost_format_table", () => {
  assert.equal(costText(0.007), "이번 단위 0.007 USD");
  assert.equal(costText(0.42), "이번 단위 0.420 USD");
  assert.equal(costText(12.5), "이번 단위 12.50 USD");
});

test("tally_with_and_without_skipped", () => {
  assert.equal(tallyText(run()), "통과 1 · 미통과 0 · 상한 1");
  assert.equal(tallyText(run({ skipped: 2 })), "통과 1 · 미통과 0 · 상한 1 · 건너뜀 2");
  assert.equal(tallyText(run({ skipped: 2, stopped: 1 })), "통과 1 · 미통과 0 · 상한 1 · 건너뜀 2 · 중단 1");
  assert.equal(tallyText(run({ stopped: 1 })), "통과 1 · 미통과 0 · 상한 1 · 중단 1");
});

test("wait_same_day_and_next_day", () => {
  const now = local(2026, 11, 5, 9, 30);
  assert.equal(waitText(null, now), null);
  assert.equal(waitText(local(2026, 11, 5, 14, 5), now), "14:05 까지");
  assert.equal(waitText(local(2026, 11, 6, 14, 5), now), "11월 6일 14:05 까지");
  assert.equal(lineText(run({ waitUntil: local(2026, 11, 5, 23, 59) }), now).wait, "23:59 까지");
});

test("state_labels_all", () => {
  const rows = {
    running: "도는 중",
    waiting_limit: "사용량 한도 대기",
    waiting_stopline: "멈출 선 대기",
    waiting_precheck: "시험 환경 대기",
    paused: "일시정지",
    taken_over: "터미널로 이어받음",
    starting: "출발 중",
    done: "끝남",
    stopped: "중단됨",
    error: "오류로 멈춤",
    something_new: "something_new",
  };
  for (const [state, label] of Object.entries(rows)) assert.equal(stateLabel(state), label, state);
});

test("buttons_by_can_order", () => {
  assert.deepEqual(buttons(run({ can: ["stop", "pause"] })), [
    { action: "pause", label: "일시정지" },
    { action: "stop", label: "중단" },
  ]);
  assert.deepEqual(buttons(run({ can: ["stop", "resume"], state: "paused" })).map((b) => b.label), ["재개", "중단"]);
  assert.deepEqual(buttons(run({ can: [] })), []);
});

test("buttons_empty_when_ended", () => {
  assert.deepEqual(buttons(run({ ended: true, can: ["pause", "stop"] })), []);
});

test("no_em_dash_in_any_text", () => {
  const now = local(2026, 11, 5, 9, 30);
  const states = ["running", "waiting_limit", "waiting_stopline", "waiting_precheck", "paused", "taken_over", "starting", "done", "stopped", "error", "x"];
  const texts = [];
  for (const state of states) {
    const sample = run({ state, skipped: 1, stopped: 1, waitUntil: local(2026, 11, 7, 1, 0) });
    texts.push(...Object.values(lineText(sample, now)).filter((v) => typeof v === "string"));
    texts.push(...buttons(sample).map((b) => b.label));
  }
  texts.push(clockText(0), clockText(3780), costText(0.007), costText(12.5), waitText(local(2026, 11, 5, 14, 5), now));
  assert.ok(texts.length > 50);
  for (const text of texts) assert.ok(!text.includes("—"), text);
});
