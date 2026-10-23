<script lang="ts">
  // 캐릭터 8종 × 기분 4가지를 한눈에 보는 개발용 화면 — 그림·동작·말풍선을 macOS 그림과 맞춰 볼 때 쓴다
  import type { Group, Limit, MascotKind, Mood, Session, State } from "$lib/types";
  import Bubble from "$lib/mascot/Bubble.svelte";
  import Figure from "$lib/mascot/Figure.svelte";
  import Foreman from "$lib/mascot/Foreman.svelte";
  import { MASCOTS } from "$lib/mascot/mascots";

  // 고정 프레임 — 맞춰 보기용으로 macOS 그림을 같은 t 로 뽑은 격자와 열이 같다
  const COLS: { mood: Mood; t: number; since: number; blink: boolean; label: string }[] = [
    { mood: "urgent", t: 0.1, since: 10, blink: false, label: "U .1" },
    { mood: "urgent", t: 0.45, since: 10, blink: false, label: "U .45" },
    { mood: "busy", t: 0.1, since: 10, blink: false, label: "B .1" },
    { mood: "busy", t: 0.45, since: 10, blink: false, label: "B .45" },
    { mood: "idle", t: 0.1, since: 10, blink: false, label: "I .1" },
    { mood: "idle", t: 0.45, since: 10, blink: false, label: "I .45" },
    { mood: "sleepy", t: 0.1, since: 10, blink: false, label: "S .1" },
    { mood: "sleepy", t: 1.3, since: 10, blink: false, label: "S 1.3" },
    { mood: "idle", t: 0.1, since: 0.2, blink: false, label: "hop" },
    { mood: "idle", t: 0.1, since: 0.53, blink: false, label: "land" },
    { mood: "busy", t: 0.1, since: 10, blink: true, label: "blink" },
  ];

  let now = $state(Date.now());
  $effect(() => {
    const id = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(id);
  });

  const session = (state: State, ago: number, topic = ""): Session => ({
    id: `${state}-${ago}`,
    cwd: "C:\\Users\\dev\\claude-board",
    state,
    since: now - ago,
    updated: now - ago,
    headless: false,
    title: topic || "요청 전",
    topic,
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
    // 세션 짝짓기 입력(SPEC §5.12, W2) — 그리기 시험 화면은 콘솔·탭이 없어 전부 null 로 둔다
    tty: null,
    tabAncestor: null,
    mode: null,
    appName: null,
  });
  const group = (s: Session): Group[] => [{ id: s.cwd, name: "claude-board", path: "~\\claude-board", sessions: [s] }];

  // 기분마다 그 기분이 나오는 세션 목록
  const MOODS: Mood[] = ["urgent", "busy", "idle", "sleepy"];
  const data = $derived<Record<Mood, { groups: Group[]; limits: Limit[] }>>({
    urgent: { groups: group(session("waiting", 60_000, "로그인 화면 고치기")), limits: [] },
    busy: {
      groups: group(session("working", 12 * 60_000, "테스트 추가")),
      limits: [{ id: "5h", label: "5시간", short: "5시간", tiny: "5h", pct: 85.4, resets: null }],
    },
    idle: { groups: group(session("idle", 60_000)), limits: [] },
    sleepy: { groups: group(session("idle", 31 * 60_000)), limits: [] },
  });

  // 줄마다 오른쪽 클릭으로 캐릭터를 바꿔 볼 수 있다
  let kinds = $state<MascotKind[]>(MASCOTS.map((m) => m.kind));

  // 말풍선 고정 비교 — macOS 말풍선을 같은 글로 뽑은 그림과 맞춰 본다
  const TINT: Record<Mood, string> = {
    urgent: "rgba(255,168,92,0.92)",
    busy: "rgba(115,168,255,0.92)",
    idle: "rgba(217,120,87,0.92)",
    sleepy: "rgba(115,115,115,0.92)",
  };
  const BUBBLES: { text: string; mood: Mood; compact?: boolean }[] = [
    { text: "승인 기다리는 세션이 있어요!", mood: "urgent" },
    { text: "claude-board 열일 중! 힘내!", mood: "busy" },
    { text: "claude-board 끝났어요! 확인해 볼까요?", mood: "idle" },
    { text: "다들 쉬는 중… 할 일 없나요?", mood: "sleepy" },
    { text: "claude-board: 로그인 화면 고치기 — 승인 부탁! 길게 늘어진 말", mood: "urgent" },
    { text: "claude-board 끝났어요! 확인해 볼까요?", mood: "idle", compact: true },
    { text: "claude-board: 로그인 화면 고치기 — 승인 부탁! 길게 늘어진 말이 한 줄을 넘어간다", mood: "urgent", compact: true },
  ];
</script>

<main>
  <h2>고정 프레임</h2>
  <div class="frozen" id="frozen">
    <div class="row">
      {#each COLS as c}<div class="label">{c.label}</div>{/each}
    </div>
    {#each MASCOTS as m (m.kind)}
      <div class="row">
        {#each COLS as c}
          <div class="cell">
            <div class="box"></div>
            <Figure kind={m.kind} mood={c.mood} t={c.t} sinceChange={c.since} blink={c.blink} />
          </div>
        {/each}
      </div>
    {/each}
  </div>

  <h2>말풍선 고정</h2>
  <div class="bubbles" id="bubbles">
    {#each BUBBLES as b}
      <div class="bubble-row">
        <span class="bubble-wrap" style:max-width="{b.compact ? 230 : 170}px">
          <Bubble text={b.text} tint={TINT[b.mood]} compact={b.compact} />
        </span>
        <Figure kind="tralalero" mood={b.mood} t={0.1} sinceChange={10} blink={false} compact={b.compact} />
      </div>
    {/each}
  </div>

  {#each [false, true] as compact}
    <h2>{compact ? "미니(compact)" : "움직이는 캐릭터"}</h2>
    <div class="live" id={compact ? "compact" : "live"}>
      {#each kinds as kind, i}
        <div class="row">
          {#each MOODS as mood}
            <div class="foreman-cell" class:compact>
              <Foreman groups={data[mood].groups} limits={data[mood].limits} {now} {compact} mascot={kind}
                onPickMascot={(k) => (kinds[i] = k)} />
            </div>
          {/each}
        </div>
      {/each}
    </div>
  {/each}
</main>

<style>
  :global(html, body) {
    margin: 0;
    background: linear-gradient(rgb(31, 31, 48), rgb(18, 18, 28));
    color: #fff;
    font-family: -apple-system, "Segoe UI", "Malgun Gothic", system-ui, sans-serif;
  }
  main {
    padding: 12px;
  }
  h2 {
    margin: 16px 0 6px;
    font-size: 13px;
    color: rgba(255, 255, 255, 0.6);
  }
  .frozen {
    display: inline-block;
    background: rgb(31, 31, 48);
  }
  .row {
    display: flex;
  }
  .label {
    width: 80px;
    height: 14px;
    font-size: 9px;
    text-align: center;
  }
  .cell {
    position: relative;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 80px;
    height: 72px;
  }
  .box {
    position: absolute;
    width: 46px;
    height: 44px;
    outline: 0.5px solid rgba(255, 255, 255, 0.08);
  }
  .bubbles {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 6px;
    width: 300px;
    padding: 10px;
    background: rgb(31, 31, 48);
  }
  .bubble-row {
    display: flex;
    align-items: center;
    gap: 2px;
    max-width: 100%;
  }
  .bubble-wrap {
    display: block;
    min-width: 0;
  }
  .foreman-cell {
    display: flex;
    justify-content: flex-end;
    align-items: center;
    width: 250px;
    height: 64px;
    padding-right: 16px;
  }
  .foreman-cell.compact {
    width: 290px;
    height: 44px;
  }
</style>
