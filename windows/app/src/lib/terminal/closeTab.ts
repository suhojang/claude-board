// 바깥 닫기(--close-tab)에 백엔드로 보낼 탭 화면 값(C4.1b) — mac EmbeddedTerminal.swift closeTabScreen 의
// Windows 판 중 웹뷰가 아는 값(탭 이름·바깥 열기 표지·나눔·막 뜸)만 골라 푼 것이다. 백엔드 `terminal.rs`
// `PaneApp` 의 웹뷰 꼴(`open_tab/close_handle.rs` `PaneView`)이기도 하다.
// 이 파일은 외부를 import 하지 않아 node:test 로 바로 시험한다.

import type { BoardSurface, BoardTab, LeafPhase } from "../types.ts";
import { leaves } from "./split.ts";
import { TYPED_WINDOW_MS } from "./place.ts";

/** 탭 화면 값 하나(`close_tab_panes` 명령의 인자) — id 는 화면(서피스) id 가 아니라 백엔드 터미널 id("t1")다 */
export interface PaneView {
  /** 백엔드 터미널 id("t1") — 닫기 판정은 이 id 로 화면을 닫는다 */
  id: string;
  /** 화면을 이름으로 찾을 이름들 — 잎 하나 탭의 탭 이름과 명령의 --name/-n 값 */
  names: string[];
  /** 화면이 기억하는 세션 id — 짝 세션이 없을 때(셸만 남은 화면) 답에 쓴다 */
  sessionId: string | null;
  /** 탭의 보이는 이름 — 닫기 답의 tab 칸 */
  tab: string;
  /** 바깥 열기(--open-tab)로 연 탭 */
  external: boolean;
  /** 자율주행을 이어받은 화면 — 웹뷰가 이어받기 표지를 아직 모르므로 항상 false다 */
  takeover: boolean;
  /** 탭의 화면이 둘 이상(나눈 탭) */
  split: boolean;
  /** 셸이 막 뜨는 중이거나 보드가 claude 명령을 막 넣음(TYPED_WINDOW_MS 안) */
  starting: boolean;
}

// 명령 줄을 낱말로 읽는다 — 큰따옴표 묶음(공백을 담을 수 있다)은 묶음을 벗겨 한 낱말로, 그 밖은 빈칸에서
// 끊는다. 묶음이 닫히지 않은 줄은 그 앞까지만 읽는다(quoteSessionName 이 묶는 꼴만 보면 되므로)
function unquotedWords(line: string): string[] {
  const words: string[] = [];
  let at = 0;
  while (at < line.length) {
    if (/\s/u.test(line[at])) {
      at++;
      continue;
    }
    if (line[at] === '"') {
      const close = line.indexOf('"', at + 1);
      if (close < 0) break;
      words.push(line.slice(at + 1, close));
      at = close + 1;
      continue;
    }
    let end = at + 1;
    while (end < line.length && !/\s/u.test(line[end])) end++;
    words.push(line.slice(at, end));
    at = end;
  }
  return words;
}

/** 명령의 `--name`/`-n` 값 — 낱말 수사(묶음은 벗겨 읽음)로 첫 값을 돌려준다. 플래그가 없거나 뒤에 값이 없으면 없다 */
function commandName(command: string): string | null {
  const words = unquotedWords(command);
  for (let i = 1; i < words.length; i++) {
    if (words[i - 1] === "--name" || words[i - 1] === "-n") return words[i];
  }
  return null;
}

/**
 * 탭 화면 값들 — 탭 순서대로, 탭 안 화면은 트리 순서(왼쪽·위가 앞)다. 셸이 아직 뜨지 않은 화면(termId null)은
 * 건너뛴다 — 닫을 방법이 없는 화면이고, 닫기 판정도 나눔 여부는 탭 트리에서 본다. takeover 은 웹뷰가
 * 이어받기 표지를 아직 모르므로 항상 false다.
 */
export function paneViews(
  tabs: readonly BoardTab[],
  surfaceOf: (leaf: string) => BoardSurface | undefined,
  phaseOf: (leaf: string) => LeafPhase | undefined,
  tabName: (t: BoardTab) => string,
  now: number,
): PaneView[] {
  const panes: PaneView[] = [];
  for (const tab of tabs) {
    const leafIds = leaves(tab.root);
    const single = leafIds.length === 1;
    for (const leafId of leafIds) {
      const surface = surfaceOf(leafId);
      if (!surface || surface.termId === null) continue;
      const names: string[] = [];
      if (single) names.push(tabName(tab)); // 한 화면 탭의 탭 이름 — 그 탭 claude 의 세션 이름이 된다
      const named = surface.command === null ? null : commandName(surface.command);
      if (named !== null) names.push(named);
      panes.push({
        id: surface.termId,
        names,
        sessionId: surface.sessionId,
        tab: tabName(tab),
        external: tab.external === true,
        takeover: false,
        split: !single,
        starting:
          phaseOf(leafId) === "starting"
          || (surface.typedAt != null && now - surface.typedAt < TYPED_WINDOW_MS),
      });
    }
  }
  return panes;
}

/** 화면 값 묶음의 지문 — 값(내용과 차례)이 바뀌었는지 견주는 열쇠. 화면 값은 이 모듈이 정한 필드 차례로 만드므로 직렬화가 정해져 있다 */
export function panesKey(panes: readonly PaneView[]): string {
  return JSON.stringify(panes);
}
