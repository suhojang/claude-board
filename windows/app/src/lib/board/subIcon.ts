// 서브에이전트 줄 아이콘 규칙 — Sources/ClaudeBoard/BoardViews.swift SubagentRow (3.2.1):
// 보드 상태가 working 이면 도는 호 대신 멈춘 진행 중 아이콘(ellipsis.circle.fill), 그 밖은 세션과 같은 StateIcon.
// 입력은 서브에이전트 자기 보드 상태(format.ts subState)뿐이다 — 부모 세션이 대기·유휴·종료여도 달라지지 않는다.
import type { State } from "$lib/types";

export type SubIconKind = "still" | State;

export const subIconKind = (boardState: State): SubIconKind => (boardState === "working" ? "still" : boardState);
