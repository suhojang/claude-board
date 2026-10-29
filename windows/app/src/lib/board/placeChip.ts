// 위치 칩의 글자·색·힌트 표: mac ClaudeBoard.swift PlaceChip 의 문자열을 window/docs/w2-design.tmp.md §3 에서 그대로 옮긴 것.
// 그리는 것은 PlaceChip.svelte 가 하고, 글자는 여기서만 정해 node:test 표(placeChip.test.ts)로 mac 글자와 맞춘다.
// 사용자에게 보이는 글자에는 긴 줄표를 쓰지 않는다: 문장은 쌍점·괄호·쉼표로 나눈다.
import type { SessionPlace } from "../terminal/place.ts";

/** 칩에 새기는 claude 띄운 방식(Session.mode), 대화형이면 null(호출 쪽에서 끝난 세션은 null 을 준다) */
export type ChipMode = "-p" | "--bg";

/** 칩 화면 하나(text 는 칩 글자, bg·fg 는 mac 실측 RGB("r g b"), tip 은 마우스를 올릴 때의 설명) */
export interface ChipView {
  text: string;
  bg: string;
  fg: string;
  mono: boolean;
  tip: string;
}

/**
 * 세션 자리와 모드에서 칩 화면을 정한다: 끝난 세션의 mode 는 읽지 않는다(호출 쪽 약속이지만 여기서도 끊는다).
 * 다른 터미널 세션은 앱 이름이 밝혀지기 전까지 칩을 그리지 않는다(null, mac 과 같다).
 */
export function chipView(place: SessionPlace, headless: boolean, appName: string | null, mode: ChipMode | null): ChipView | null {
  switch (place.kind) {
    case "tab":
      return {
        text: mode ? `CB ${mode}` : "CB",
        bg: "46 59 87",
        fg: "168 194 255",
        mono: false,
        // 화면 안에서 시작한 claude(headless)와 -p·--bg 는 탭이 닫히면 함께 끝난다. 그 사실을 힌트에 앞세운다
        tip: headless || mode
          ? `보드 탭 안에서 시작된 claude ${mode ?? "-p"} (탭이 닫히면 함께 끝납니다)`
          : "보드 탭에서 도는 세션: 누르면 그 탭으로",
      };
    case "terminal":
      if (!appName) return null; // 이름 모름: 칩 자리를 비운 채 기다린다(탐침 3초 사이의 잠깐)
      return { text: appName, bg: "36 64 74", fg: "143 217 232", mono: false, tip: `${appName} 에서 도는 세션` };
    case "background":
      return {
        text: `$ claude ${mode ?? "-p"}`,
        bg: "59 51 69",
        fg: "212 184 255",
        mono: true,
        tip: "터미널 없이 도는 세션입니다. 보드는 상태만 보여 줍니다",
      };
    case "ended":
      return headless
        ? { text: "$ claude -p", bg: "", fg: "", mono: true, tip: "끝난 백그라운드 세션입니다. 메뉴의 세션 다시 열기로 보드 탭에서 엽니다" }
        : { text: "보드에서 이어 열기", bg: "", fg: "", mono: false, tip: "누르면 보드 새 탭에서 이 대화를 이어 엽니다" };
  }
}