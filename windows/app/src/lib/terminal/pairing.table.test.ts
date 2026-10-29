// @ts-nocheck
// node:test 로만 돌고 svelte-check 대상이 아니다
// 실행: cd windows/app && node --test src/lib/terminal/pairing.table.test.ts
// 공유 규칙 표 rules-table/pairing.json(꼴 소유자 rules-table/README.md, 줄 설계 plan.md 2.2~2.4 의 짝짓기 T1)을
// Windows 짝짓기 코드(place.ts pairSessions·placeOf, tabLogic.ts mostUrgent)로 돌린다. 시각은 표의 초를 밀리초로 곱한다
import assert from "node:assert/strict";
import { test } from "node:test";
import type { BoardSurface, Session } from "../types.ts";
import { placeOf, pairSessions } from "./place.ts";
import { mostUrgent } from "./tabLogic.ts";
import { runTable } from "../rulesTable.ts";

// ── JSON 줄을 TS 값으로 ──────────────────────────────────────────────────────

const STATES = new Set(["working", "waiting", "idle", "paused", "ended"]);

/** 줄 객체는 꼴의 키를 정확히 쓴다: 모르는 키도, 빠진 키도 실패다(README: 모르는 키는 돌리는 쪽이 실패) */
function needExactKeys(row, keys, what) {
  if (typeof row !== "object" || row === null || Array.isArray(row)) {
    throw new Error(`${what} 은 객체여야 한다`);
  }
  for (const key of Object.keys(row)) {
    if (!keys.includes(key)) throw new Error(`${what} 에 모르는 키 '${key}' 가 있다`);
  }
  for (const key of keys) {
    if (!(key in row)) throw new Error(`${what} 에 빠진 키 '${key}' 가 있다`);
  }
}

/** JSON 세션 줄을 Session 으로. tty "" 는 tty 없음(null), 대화형(mode 없음)은 mode null, 시각은 초에서 밀리초로 */
function sessionOf(row: unknown): Session {
  needExactKeys(row, ["id", "state", "tty", "updated", "since", "headless"], `세션 줄 ${JSON.stringify(row.id)}`);
  if (typeof row.id !== "string" || row.id === "") throw new Error("세션 id 는 비지 않은 문자열이어야 한다");
  if (!STATES.has(row.state)) throw new Error(`세션 ${row.id} 의 상태 '${row.state}' 은 모르는 상태다`);
  if (typeof row.updated !== "number" || typeof row.since !== "number") {
    throw new Error(`세션 ${row.id} 의 updated·since 는 숫자(초)여야 한다`);
  }
  return {
    id: row.id,
    cwd: ".",
    pid: 0,
    state: row.state,
    since: row.since * 1000,
    updated: row.updated * 1000,
    headless: row.headless === true,
    title: "",
    topic: "",
    message: "",
    name: null,
    ctx: null,
    brain: null,
    subagents: [],
    shells: 0,
    paused: false,
    muted: false,
    limitAt: null,
    canDrive: true,
    tty: row.tty === "" ? null : row.tty,
    tabAncestor: null,
    mode: null,
    appName: null,
  };
}

/** JSON 화면 줄을 BoardSurface 로. tty "" 는 아직 셸이 없는 화면(null termId), session_id null 은 셸 화면 */
function surfaceOf(row: unknown): BoardSurface {
  needExactKeys(row, ["id", "tty", "session_id"], `화면 줄 ${JSON.stringify(row.id)}`);
  if (typeof row.id !== "string" || row.id === "") throw new Error("화면 id 는 비지 않은 문자열이어야 한다");
  if (typeof row.tty !== "string") throw new Error(`화면 ${row.id} 의 tty 는 문자열이어야 한다`);
  if (row.session_id !== null && typeof row.session_id !== "string") {
    throw new Error(`화면 ${row.id} 의 session_id 는 세션 id 문자열이나 null 이어야 한다`);
  }
  return {
    id: row.id,
    termId: row.tty === "" ? null : row.tty,
    sessionId: row.session_id,
    command: null,
    held: null,
    mdRenderOff: false,
    title: "",
    typedAt: null,
  };
}

/** 화면 고침을 복사 화면에 적는다(hub.svelte.ts 가 적용한 뒤 상태, place.test.ts 의 surfaceAfter 와 같은 순서) */
function surfacesAfter(surfaces: BoardSurface[], changes: ReturnType<typeof pairSessions>["changes"]): BoardSurface[] {
  const after = surfaces.map((surface) => ({ ...surface }));
  for (const change of changes) {
    const target = after.find((each) => each.id === change.surfaceId);
    if (!target) throw new Error(`모르는 화면 id 의 고침: ${JSON.stringify(change)}`);
    if (change.kind === "sessionId") target.sessionId = change.sessionId;
    else if (change.kind === "command") target.command = change.command;
    else {
      target.command = null;
      target.mdRenderOff = false;
    }
  }
  return after;
}

/** SessionPlace 를 표의 자리 문자열로. 표 꼴은 headless 표지를 자리 문자열에 담지 않는다(README) */
function placeText(place): string {
  if (place.kind === "tab") return `tab:${place.surfaceId}`;
  return place.kind;
}

// ── 규칙 판정 함수 ───────────────────────────────────────────────────────────

/** screen-session: 짝짓기 후 화면이 기억하는 세션 id(고침 적용 뒤)와 짝 표 */
function screenSessionRule(c) {
  needExactKeys(c.input, ["screens", "sessions"], "screen-session 입력");
  needExactKeys(c.expect, ["screen_session", "paired"], "screen-session 기대");
  const sessions = c.input.sessions.map(sessionOf);
  const surfaces = c.input.screens.map(surfaceOf);
  const plan = pairSessions({ sessions, surfaces, runsClaude: [], now: 0 });
  const screenSession = {};
  for (const surface of surfaces) screenSession[surface.id] = surface.sessionId;
  for (const change of plan.changes) {
    if (change.kind === "sessionId") screenSession[change.surfaceId] = change.sessionId;
  }
  assert.deepEqual(screenSession, c.expect.screen_session);
  assert.deepEqual(plan.paired, c.expect.paired);
}

/** place: 짝짓기 뒤 화면 고침을 적용하고 placeOf 로 세션 자리를 본다(hub 가 changes 를 적용한 뒤 부르는 순서) */
function placeRule(c) {
  needExactKeys(c.input, ["screens", "sessions", "runs_claude", "owned"], "place 입력");
  needExactKeys(c.expect, ["place"], "place 기대");
  const sessions = c.input.sessions.map(sessionOf);
  const surfaces = c.input.screens.map(surfaceOf);
  // runs_claude 는 화면 id 목록이고 짝짓기 코드는 tty(termId)로 본다: 화면으로 바꾸고, 없는 화면이면 실패
  const runsClaude = (c.input.runs_claude ?? []).map((screenId) => {
    const surface = surfaces.find((each) => each.id === screenId);
    if (!surface) throw new Error(`runs_claude 의 화면 '${screenId}' 이 화면 목록에 없다`);
    if (surface.termId === null) throw new Error(`runs_claude 의 화면 '${screenId}' 은 셸이 없다`);
    return surface.termId;
  });
  const plan = pairSessions({ sessions, surfaces, runsClaude, now: 0 });
  const after = surfacesAfter(surfaces, plan.changes);
  // owned 는 표에서 세션 id 목록(화면 안 headless)으로 오고 코드에서는 세션 id → true 표다
  const owned = { ...plan.owned };
  for (const sessionId of c.input.owned ?? []) {
    if (typeof sessionId !== "string" || sessionId === "") {
      throw new Error("owned 는 비지 않은 세션 id 문자열 목록이어야 한다");
    }
    owned[sessionId] = true;
  }
  const place = {};
  for (const session of sessions) {
    const got = placeOf(session, { paired: plan.paired, owned }, after, runsClaude, 0);
    place[session.id] = placeText(got);
  }
  assert.deepEqual(place, c.expect.place);
}

/** workspace-dot: 경로 줄의 점은 상태 목록의 가장 급한 것(mostUrgent) */
function workspaceDotRule(c) {
  needExactKeys(c.input, ["states"], "workspace-dot 입력");
  needExactKeys(c.expect, ["state"], "workspace-dot 기대");
  if (!Array.isArray(c.input.states)) throw new Error("상태 목록(states)은 배열이어야 한다");
  for (const state of c.input.states) {
    if (!STATES.has(state)) throw new Error(`상태 목록의 '${state}' 은 모르는 상태다`);
  }
  assert.equal(mostUrgent(c.input.states), c.expect.state);
}

// ── 표 돌리기 ────────────────────────────────────────────────────────────────

test("공유 표 rules-table/pairing.json: 모든 줄이 Windows 짝짓기 코드로 기대대로 돈다", () => {
  runTable("pairing", {
    "screen-session": screenSessionRule,
    "place": placeRule,
    "workspace-dot": workspaceDotRule,
  });
});