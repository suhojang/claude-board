// @ts-nocheck — node:test 로만 돌고 svelte-check 대상이 아니다
// 위치 칩의 글자·색·힌트가 mac PlaceChip(ClaudeBoard.swift)과 같은지 시험한다 — design §3 문자열 표이다.
// 실행: cd windows/app && node --test src/lib/board/placeChip.test.ts   (Node 22.18+/24 는 .ts 를 바로 읽는다)
import assert from "node:assert/strict";
import { test } from "node:test";

import { chipView, type ChipMode, type ChipView } from "./placeChip.ts";
import type { SessionPlace } from "../terminal/place.ts";

const tab = (surfaceId = "srf1"): SessionPlace => ({ kind: "tab", surfaceId, headless: true });
const terminal: SessionPlace = { kind: "terminal" };
const background: SessionPlace = { kind: "background" };
const ended: SessionPlace = { kind: "ended" };

const endedHeadlessChip: ChipView = { text: "$ claude -p", bg: "", fg: "", mono: true, tip: "끝난 백그라운드 세션입니다. 메뉴의 세션 다시 열기로 보드 탭에서 엽니다" };
const endedInteractiveChip: ChipView = { text: "보드에서 이어 열기", bg: "", fg: "", mono: false, tip: "누르면 보드 새 탭에서 이 대화를 이어 엽니다" };

// design §3 표의 행 — expected 는 칩 전체(text·bg·fg·mono·tip)를 통째로 비교해 mac 글자 하나라도 다르면 빨개진다
type Row = {
  name: string;
  place: SessionPlace;
  headless: boolean;
  appName: string | null;
  mode: ChipMode | null;
  expected: ChipView | null;
};

const rows: Row[] = [
  {
    name: "탭의 대화형 세션은 CB 칩이고 탭으로 가는 힌트(쌍점)를 내린다",
    place: tab(), headless: false, appName: null, mode: null,
    expected: { text: "CB", bg: "46 59 87", fg: "168 194 255", mono: false, tip: "보드 탭에서 도는 세션: 누르면 그 탭으로" },
  },
  {
    name: "탭에서 시작한 headless 세션은 탭과 함께 끝난다는 힌트를 앞세운다",
    place: tab(), headless: true, appName: null, mode: null,
    expected: { text: "CB", bg: "46 59 87", fg: "168 194 255", mono: false, tip: "보드 탭 안에서 시작된 claude -p (탭이 닫히면 함께 끝납니다)" },
  },
  {
    name: "탭의 -p 세션은 CB -p 칩이 된다",
    place: tab(), headless: true, appName: null, mode: "-p",
    expected: { text: "CB -p", bg: "46 59 87", fg: "168 194 255", mono: false, tip: "보드 탭 안에서 시작된 claude -p (탭이 닫히면 함께 끝납니다)" },
  },
  {
    name: "탭의 --bg 세션은 CB --bg 칩이 된다",
    place: tab(), headless: true, appName: null, mode: "--bg",
    expected: { text: "CB --bg", bg: "46 59 87", fg: "168 194 255", mono: false, tip: "보드 탭 안에서 시작된 claude --bg (탭이 닫히면 함께 끝납니다)" },
  },
  {
    name: "탭의 -p 세션은 headless 가 아니어도 함께 끝난다는 힌트를 내린다(모드만으로도 안다)",
    place: tab(), headless: false, appName: null, mode: "-p",
    expected: { text: "CB -p", bg: "46 59 87", fg: "168 194 255", mono: false, tip: "보드 탭 안에서 시작된 claude -p (탭이 닫히면 함께 끝납니다)" },
  },
  {
    name: "탭의 --bg 세션도 headless 가 아니어도 함께 끝난다는 힌트를 내린다",
    place: tab(), headless: false, appName: null, mode: "--bg",
    expected: { text: "CB --bg", bg: "46 59 87", fg: "168 194 255", mono: false, tip: "보드 탭 안에서 시작된 claude --bg (탭이 닫히면 함께 끝납니다)" },
  },
  {
    name: "다른 터미널 세션은 앱 이름 칩을 내린다",
    place: terminal, headless: false, appName: "Windows Terminal", mode: null,
    expected: { text: "Windows Terminal", bg: "36 64 74", fg: "143 217 232", mono: false, tip: "Windows Terminal 에서 도는 세션" },
  },
  {
    name: "다른 터미널 세션은 앱 이름이 밝혀지기 전까지 칩을 그리지 않는다",
    place: terminal, headless: false, appName: null, mode: null,
    expected: null,
  },
  {
    name: "다른 터미널 세션은 빈 앱 이름도 이름 모름으로 쳐서 칩을 그리지 않는다",
    place: terminal, headless: false, appName: "", mode: null,
    expected: null,
  },
  {
    name: "터미널 없는 세션은 $ claude -p 고정표 칩을 내린다",
    place: background, headless: true, appName: null, mode: null,
    expected: { text: "$ claude -p", bg: "59 51 69", fg: "212 184 255", mono: true, tip: "터미널 없이 도는 세션입니다. 보드는 상태만 보여 줍니다" },
  },
  {
    name: "터미널 없는 --bg 세션은 모드를 그대로 새긴다",
    place: background, headless: true, appName: null, mode: "--bg",
    expected: { text: "$ claude --bg", bg: "59 51 69", fg: "212 184 255", mono: true, tip: "터미널 없이 도는 세션입니다. 보드는 상태만 보여 줍니다" },
  },
  {
    name: "끝난 백그라운드 세션은 색을 비우고 되살리는 길을 힌트로 내린다",
    place: ended, headless: true, appName: null, mode: null,
    expected: endedHeadlessChip,
  },
  {
    name: "끝난 대화형 세션은 이어 열기 칩이고 등폭 글자다",
    place: ended, headless: false, appName: null, mode: null,
    expected: endedInteractiveChip,
  },
];

test("design §3 의 칩 문자열·색 표를 통째로 맞춘다", () => {
  for (const row of rows) {
    assert.deepEqual(chipView(row.place, row.headless, row.appName, row.mode), row.expected, row.name);
  }
});

test("끝난 세션의 mode 는 읽지 않는다(호출 쪽이 null 을 주든 --bg 를 주든 같은 칩)", () => {
  assert.deepEqual(chipView(ended, true, null, "--bg"), endedHeadlessChip);
  assert.deepEqual(chipView(ended, false, null, "--bg"), endedInteractiveChip);
});

test("사용자에게 보이는 칩 글자에 긴 줄표(—)를 쓰지 않는다", () => {
  const places: SessionPlace[] = [tab(), terminal, background, ended];
  const modes: (ChipMode | null)[] = [null, "-p", "--bg"];
  for (const place of places) {
    for (const headless of [false, true]) {
      for (const appName of [null, "Windows Terminal"]) {
        for (const mode of modes) {
          const view = chipView(place, headless, appName, mode) as ChipView | null;
          if (!view) continue;
          assert.ok(!view.text.includes("—"), `${view.text}`);
          assert.ok(!view.tip.includes("—"), `${view.tip}`);
        }
      }
    }
  }
});