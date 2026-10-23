// 세션과 보드 터미널 화면(서피스)을 짝짓고 세션이 어디서 도는지(위치 조형)를 가르는 순수 함수다.
// mac ClaudeBoard.swift TerminalHub.pair/place 의 Windows 판 — hub.svelte.ts 가 화면을 쥔 상태에서 부른다.
// Svelte 를 모르고 입력을 고치지 않는다 — 화면 고침은 계획(changes)으로만 돌려주므로 node:test 로 바로 시험한다.
import type { BoardSurface, Session, TermId } from "../types.ts";

// 세션이 도는 곳 — 칩(PlaceChip)과 카드가 읽는다(mac SessionPlace). types.ts 의 옛 조형(appName·mode 를 안에 두는 것)을
// 대신하는 새 조형이다; types.ts 의 교체는 W2d(main 이 쥐는 파일)가 하고, 이 모듈은 여기서 계약 전체를 쥔다.
export type SessionPlace =
  | { kind: "tab"; surfaceId: string; headless: boolean } // 보드 탭 화면 소속 — headless 는 그 화면 안에서 시작된 claude -p
  | { kind: "terminal" } // 보드 밖 다른 터미널 앱 — 앱 이름은 Session.appName 이 따로 전한다
  | { kind: "background" } // 터미널 없이 도는 세션($ claude -p · --bg)
  | { kind: "ended" }; // 끝난 세션

/** hub 가 화면에 적용할 고침 하나 — hub.svelte.ts 의 mutator 로만 적용한다(pairSessions 는 화면을 고치지 않는다) */
export type SurfaceChange =
  | { kind: "sessionId"; surfaceId: string; sessionId: string } // 화면의 세션 id 를 붙이거나 따라간다(/clear · 셸에서 사람이 띄운 claude)
  | { kind: "command"; surfaceId: string; command: string } // 셸 화면에 짝 세션의 claude 명령을 새긴다 — claude 화면이 된다
  | { kind: "reset"; surfaceId: string }; // 명령을 비우고 꾸미기 끔을 푼다 — claude 가 끝나 셸 화면으로 돌아간다

/** 짝짓기 상태(mac TerminalHub.paired·tabOwnedHeadless) — placeOf 가 읽고 hub 가 상태로 둔다 */
export interface SessionPairing {
  paired: Record<string, string>; // 세션 id -> 화면 id
  owned: Record<string, true>; // 화면 안에서 시작한 headless 세션만 — 칩 "CB -p" 의 headless 근거
}

/** pairSessions 의 결과 — hub 가 changes 를 적용한 뒤 paired·owned 를 상태로 두고 placeOf 에 준다 */
export interface SurfacePlan extends SessionPairing {
  changes: SurfaceChange[];
}

/** 짝짓기 입력 — runsClaude 는 스냅샷의 선택 필드라 값 자체가 없거나 null 이면 빈 것으로 본다 */
export interface PairInput {
  sessions: Session[]; // 세션 파일 묶음
  surfaces: BoardSurface[]; // 지금 열린 모든 보드 화면(탭 트리의 잎)
  runsClaude?: TermId[] | null; // 지금 claude 가 도는 보드 터미널
  now?: number; // 지금 시각(ms) — typedAt 창을 재는 기준, 비우면 Date.now()
}

/** 셸에 claude 명령을 친 지 이만큼은 claude 가 도는 것으로 본다(mac runsClaude 의 typedAt < 3초) */
export const TYPED_WINDOW_MS = 3000;

/** 화면에 방금(3초 안) claude 명령을 쳤나 — 프로세스 스냅샷이 따라오기 전의 찰나를 메운다 */
const typedRecently = (surface: BoardSurface, now: number): boolean =>
  surface.typedAt != null && now - surface.typedAt < TYPED_WINDOW_MS;

/**
 * 세션을 화면들과 짝짓는다 — mac ClaudeBoard.swift TerminalHub.pair 의 순수 판. 끝난 세션은 짝짓지 않는다:
 * /clear 로 id 만 바뀐 세션이 같은 콘솔(pid)을 다시 쓰므로, 끝난 옛 id 를 화면에 묶어 두면 잘못된 자리가 된다.
 *
 * 1.(tty 짝) 사는 세션의 tty 가 화면 termId 와 같으면 그 화면 소속(paired)이고 화면의 주 세션(main)이다.
 *   화면 sessionId 가 다르면 sessionId 고침을 내놓는다(/clear 로 id 가 바뀌었거나 셸에서 사람이 띄운 claude 도
 *   그 화면의 세션 id 가 따라간다).
 * 2.(탭 자손) 사는 headless 세션의 tabAncestor(조상)가 화면 termId 를 가르키면 그 화면 소속 + owned
 *   (화면이 닫히면 함께 끝난다 — claude 가 Bash 도구로 띄운 claude -p).
 * 3.(화면 고침) 명령이 없는 셸 화면에 주 세션(mode 없는 대화형)이 짝지어 오면 command = "claude" 고침; 주 세션도 없고
 *   runsClaude 도 없어 돌지 않는 화면에 명령이 남아 있으면 명령 비움 + 꾸미기 끔 품음(셸 화면으로 돌아간다) 고침.
 */
export function pairSessions({ sessions, surfaces, runsClaude, now = Date.now() }: PairInput): SurfacePlan {
  const runningTerms = new Set<TermId>(runsClaude ?? []);
  const paired: Record<string, string> = {};
  const owned: Record<string, true> = {};
  const changes: SurfaceChange[] = [];

  // termId 로 화면을 찾는 표(mac byTty). 화면 하나가 termId 하나를 쥐므로 같은 termId 의 화면이 둘일 일은 없다
  const byTermId = new Map<TermId, BoardSurface>();
  for (const surface of surfaces) {
    if (surface.termId !== null) byTermId.set(surface.termId, surface);
  }

  // 규칙 1 — tty 짝과 화면의 주 세션
  const main = new Map<string, Session>(); // 화면 id -> 그 화면에서 도는 주 세션
  for (const session of sessions) {
    if (session.state === "ended" || !session.tty) continue;
    const surface = byTermId.get(session.tty);
    if (!surface) continue; // tty 가 모르는 termId 를 가르키면 짝짓지 않는다
    paired[session.id] = surface.id;
    main.set(surface.id, session);
    if (surface.sessionId !== session.id) {
      changes.push({ kind: "sessionId", surfaceId: surface.id, sessionId: session.id });
    }
  }

  // 규칙 2 — 탭 자손 headless 세션은 그 화면 소속이고 탭의 주인이다
  for (const session of sessions) {
    if (session.state === "ended" || !session.headless || !session.tabAncestor) continue;
    const surface = byTermId.get(session.tabAncestor);
    if (!surface) continue;
    paired[session.id] = surface.id;
    owned[session.id] = true;
  }

  // 규칙 3 — 화면마다 명령 새기기·비우기. 돌고 있음 = 주 세션이 있거나 그 termId 에서 claude 가 도는 중(runsClaude)
  for (const surface of surfaces) {
    const session = main.get(surface.id);
    const running =
      session !== undefined
      || (surface.termId !== null && runningTerms.has(surface.termId))
      || typedRecently(surface, now);
    // 셸 프롬프트에서 사람이 친 대화형 claude(mode 없음)도 claude 화면으로 끝나면 이어 열린다(-p·--bg 는 뺀다)
    if (surface.command === null && session !== undefined && (session.mode ?? null) === null) {
      changes.push({ kind: "command", surfaceId: surface.id, command: "claude" });
    }
    if (!running && surface.command !== null) {
      // 보드가 넣은 claude 가 끝나 셸만 남았다 — 셸 화면으로 돌아간다(끈 꾸미기 기억은 hub 가 남긴다)
      changes.push({ kind: "reset", surfaceId: surface.id });
    }
  }
  return { paired, owned, changes };
}

/**
 * 세션이 어디서 도는지 가른다 — mac ClaudeBoard.swift TerminalHub.place. 자리는 세 가지로 본다:
 * 짝짓기 표(paired) · 화면이 기억하는 세션 id(끝난 세션은 그 화면에서 claude 가 막 뜨는 중(runsClaude)일 때만
 * 탭 소속 — 이어 열기를 넣은 직후의 찰나) · 그 밖은 ended·background·terminal.
 *
 * pairing 은 pairSessions 의 결과(또 hub 가 적용하고 남긴 paired·owned 상태다). surfaces 는 hub 가 changes 를
 * 적용한 뒤의 화면들(화면 sessionId 가 따라간 뒤)이어야 옛 id·새 id 를 정확히 가른다.
 */
export function placeOf(
  session: Session,
  pairing: SessionPairing,
  surfaces: BoardSurface[],
  runsClaude?: TermId[] | null,
  now: number = Date.now(),
): SessionPlace {
  const runningTerms = new Set<TermId>(runsClaude ?? []);
  const held = surfaces.find(
    (surface) =>
      surface.sessionId === session.id
      && (session.state !== "ended"
        || (surface.termId !== null && runningTerms.has(surface.termId))
        || typedRecently(surface, now)),
  );
  const surfaceId: string | undefined = pairing.paired[session.id] ?? held?.id;
  if (surfaceId !== undefined) {
    // 화면 소속 — headless 는 규칙 1·2 에서 그 화면의 주인이 되었을 때만(mac .tab(UUID, headless:))
    return { kind: "tab", surfaceId, headless: pairing.owned[session.id] === true };
  }
  if (session.state === "ended") return { kind: "ended" };
  return session.headless ? { kind: "background" } : { kind: "terminal" };
}