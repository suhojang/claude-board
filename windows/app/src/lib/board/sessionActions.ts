// 세션 줄이 부르는 동작 — 메뉴 항목이 내는 동작 이름을 한 곳에서 정한다. 이름은 mac performAction 의 어휘
// (pause·resume·stop·reopen·focus)와 Windows 전용 activate 뿐이다(Rust session_action.rs 의 SessionAction::parse 와 같은 집합).
// 메뉴 문구·설명은 mac SessionMenuPanel.items(for:place:) 와 같다. 외부를 import 하지 않아(타입만) node:test 로 시험한다.
import type { Session, SessionAction } from "../types.ts";
import type { SessionPlace } from "../terminal/place.ts";

/** 백엔드 session_action 이 아니라 프론트가 흐름을 짜는 동작(보드로 옮기기 — mac moveToBoard) */
export type MenuAction = SessionAction | "moveToBoard";

export interface SessionCall {
  action: MenuAction;
  /** reopen 에만 쓴다 — true 면 "이어서 진행" 문구를 첫 요청으로 넘긴다 */
  carryOn?: boolean;
}

/** detail 은 항목 아래에 보이는 한 줄 설명(mac Item.detail) */
export type SessionMenuEntry = { separator: true } | ({ label: string; detail: string; enabled?: boolean } & SessionCall);

type MenuSession = Pick<Session, "state" | "paused" | "canDrive" | "headless">;

const MOVE_ENTRY: SessionMenuEntry = { label: "보드로 옮기기", detail: "그쪽을 끝내고 보드 새 탭에서 이어 엽니다", action: "moveToBoard" };

const STOP_ENTRY: SessionMenuEntry = { label: "세션 중단", detail: "응답을 멈추고 끝냅니다. 대화 기록은 남습니다", action: "stop" };

function pauseDetail(s: MenuSession, canPause: boolean): string {
  if (canPause) return "지금 응답을 멈춥니다. 재개하면 이어 갑니다";
  return s.state === "idle" ? "대기 중이라 멈출 응답이 없습니다" : "이 터미널에는 키를 넣을 수 없습니다";
}

// 끝난 세션은 이어 열기, 일시중지는 재개·중단, 그 밖은 일시중지·(다른 터미널이면) 보드로 옮기기·중단. 터미널 없이 도는 살아 있는 세션은 신호를 보내지 않으므로 메뉴가 없다
export function sessionMenuEntries(s: MenuSession, place: SessionPlace): SessionMenuEntry[] {
  if (s.state === "ended") {
    if (s.paused) {
      return [{ label: "세션 재개 (이어서 진행)", detail: "보드 새 탭에서 열고 멈춘 작업을 이어 갑니다", action: "reopen", carryOn: true }];
    }
    const detail = s.headless ? "보드 새 탭에서 대화형으로 이어 엽니다" : "보드 새 탭에서 이 대화를 이어 엽니다";
    return [{ label: "세션 다시 열기", detail, action: "reopen", carryOn: false }];
  }
  if (place.kind === "background") return [];
  if (s.state === "paused") {
    return [{ label: "세션 재개", detail: "멈춘 지점부터 이어서 진행하게 합니다", action: "resume" }, { separator: true }, { ...STOP_ENTRY }];
  }
  const canPause = s.state !== "idle" && s.canDrive;
  // 다른 터미널의 세션은 그쪽을 /exit 로 끝내고 보드 탭에서 이어 연다 — 키를 넣을 수 있을 때만(mac place == .terminal, canDrive)
  const move = place.kind === "terminal" && s.canDrive ? [{ ...MOVE_ENTRY }] : [];
  return [
    { label: "일시중지", detail: pauseDetail(s, canPause), enabled: canPause, action: "pause" },
    ...move,
    { separator: true },
    { ...STOP_ENTRY },
  ];
}
