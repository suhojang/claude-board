// 창 메뉴 항목의 글자·그림 선택 — 순수 함수. mac 의 윈도우 메뉴(SettingsScreen.swift 의 snapAction 항목과 validateMenuItem)와
// 제목 막대 붙이기 단추(BoardTiers.swift SnapButton)가 같은 기준을 쓴다: 붙이기 항목의 글자·그림은 보드 위치(boardSide)만 따른다.
// 접힘·펼침·미니·항상 위는 글자를 바꾸지 않는다(체크 표시만 바뀐다). boardSide 가 "left" 가 아닌 모든 값(없음·알 수 없는 값)은 오른쪽이다
// (mac: `UserDefaults.string(forKey: "boardSide") == "left"` 만 왼쪽).
import type { BoardSide } from "$lib/types";

export interface WindowState {
  side: BoardSide | string | null | undefined;
  mini: boolean;
  expanded: boolean; // 터미널 펼침
  pinned: boolean; // 항상 위
}

export interface MenuEntry {
  id: "snap" | "terminal" | "pin";
  label: string;
  checked?: boolean; // 체크 표시가 있는 항목만(터미널 펼치기·항상 위)
}

const isLeft = (side: WindowState["side"]) => side === "left";

/** 메뉴 항목 글자 — mac `처음 크기로 오른쪽(왼쪽)에 붙이기` */
export function snapMenuLabel(side: WindowState["side"]): string {
  return isLeft(side) ? "처음 크기로 왼쪽에 붙이기" : "처음 크기로 오른쪽에 붙이기";
}

/** 제목 막대 단추 안내 — mac `처음 크기로 되돌려 화면 왼쪽(오른쪽)에 붙이기 (단축키)` */
export function snapHint(side: WindowState["side"], key?: string | null): string {
  return `처음 크기로 되돌려 화면 ${isLeft(side) ? "왼쪽" : "오른쪽"}에 붙이기${key ? ` (${key})` : ""}`;
}

/** 단추 그림 — mac `rectangle.lefthalf/righthalf.inset.filled`. 윈도우 판은 Icon.svelte 의 기존 그림 이름을 쓴다 */
export function snapIconName(side: WindowState["side"]): "panel-left" | "snap-right" {
  return isLeft(side) ? "panel-left" : "snap-right";
}

/** 창 메뉴(트레이 오른쪽 클릭 메뉴의 창 관련 항목) — 글자는 고정이 아니라 위 함수에서 온다 */
export function windowMenuItems(s: WindowState): MenuEntry[] {
  return [
    { id: "snap", label: snapMenuLabel(s.side) },
    { id: "terminal", label: "터미널 펼치기", checked: s.expanded },
    { id: "pin", label: "항상 위에 고정", checked: s.pinned },
  ];
}
