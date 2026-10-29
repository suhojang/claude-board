<script lang="ts">
  // 보드 윗줄(ClaudeBoard.swift BoardView 윗줄) — 앱 아이콘·이름·버전, "활성 세션 n개 · Claude Code v", 오른쪽 캐릭터.
  // 둘째 줄은 늘 상태 칩(왼쪽)과 단추(오른쪽) — 폭에 따라 한 줄·두 줄을 바꾸면 "알림 꺼짐" 이 생길 때마다 단추 줄이 오간다
  import { scale } from "svelte/transition";
  import type { Snapshot, State } from "$lib/types";
  import Foreman from "$lib/mascot/Foreman.svelte";
  import { checkUpdate, reveal, setAllCollapsed, setAllMuted, setMascot, setPinned, setUsageCompact } from "$lib/backend";
  import Fit from "./Fit.svelte";
  import Icon from "./Icon.svelte";
  import { stateName, stateRgb } from "./format";
  import { hint } from "./hint.svelte";
  import { ui } from "./store.svelte";

  let { snap, full, onScrollTo }: { snap: Snapshot; full: boolean; onScrollTo: (id: string) => void } = $props();

  const active = $derived(snap.groups.reduce((n, g) => n + g.sessions.filter((s) => s.state !== "ended").length, 0));
  const pinned = $derived(snap.settings.pinned);
  const muted = $derived(snap.allMuted);
  const checking = $derived(snap.updater.checking);
  // 모두 접기 = 경로 카드를 다 접고 사용 한도를 간략(한 줄)으로, 모두 펼치기 = 다 펴고 상세로
  const allFolded = $derived(
    snap.settings.usageCompact && snap.groups.every((g) => snap.settings.collapsed.includes(g.id)),
  );

  // 칩 한 번 = 그 상태만 보기(해당 카드를 펼치고 맨 위로), 같은 칩 다시 = 모두 보기
  function chipTapped(st: State) {
    if (ui.filter === st) {
      ui.filter = null;
      return;
    }
    const ids = snap.groups.filter((g) => g.sessions.some((s) => s.state === st)).map((g) => g.id);
    ui.filter = st;
    ids.forEach(reveal);
    if (ids[0]) onScrollTo(ids[0]);
  }

  function foldAll() {
    setAllCollapsed(!allFolded);
    setUsageCompact(!allFolded);
  }
</script>

<div class="header">
  <div class="top">
    <div class="title">
      <div class="name">
        <img src="/icon.png" alt="" width="24" height="24" />
        <span class="app">Claude Board</span>
        <span class="ver" use:hint={"Claude Board 버전"}>{snap.updater.dev ? "dev" : snap.updater.current}</span>
      </div>
      <div class="sub">
        <span class="num">활성 세션 {active}개</span>
        {#if snap.claudeVersion}
          <!-- Claude Code 버전은 부가 정보라 세션 수 옆에 흐리게 붙인다 -->
          <span class="cc" use:hint={"설치된 Claude Code 버전"}>&nbsp;· Claude Code {snap.claudeVersion}</span>
        {/if}
      </div>
    </div>
    <div class="mascot">
      <Foreman groups={snap.groups} limits={snap.usage.limits} now={snap.now} mascot={snap.settings.mascot} onPickMascot={setMascot} />
    </div>
  </div>

  <!-- 윗줄이 좁으면 "위에 고정" 글자를, 더 좁으면 "알림 꺼짐" 글자까지 빼고 아이콘만 -->
  <Fit count={3} class="controlsFit">
    {#snippet content(level)}
      <div class="controls">
        <div class="chips">
          {#each ["waiting", "working", "idle"] as const as st (st)}
            {#if snap.counts[st] > 0}
              {@const filtering = ui.filter === st}
              <button
                class="chip num"
                class:filtering
                style:--c={stateRgb(st)}
                onclick={() => chipTapped(st)}
                transition:scale={{ start: 0.6, duration: 200 }}
                use:hint={filtering ? `${stateName(st)}만 보는 중 — 누르면 모두 보기` : `${stateName(st)} ${snap.counts[st]}개 — 누르면 ${stateName(st)}만 보기`}
              >
                <span class="dot"></span>{snap.counts[st]}
              </button>
            {/if}
          {/each}
        </div>
        <span class="spacer"></span>
        <button
          class="pill"
          style:width={muted && level < 2 ? undefined : "28px"}
          class:muted
          onclick={() => setAllMuted(!muted)}
          use:hint={muted
            ? "전체 알림 꺼짐 — 누르면 다시 켜고, 세션마다 정해 둔 알림 설정으로 돌아갑니다"
            : "전체 알림 끄기 — 모든 세션의 작업 완료·승인 대기 알림을 띄우지 않습니다"}
        >
          <Icon name={muted ? "bell-slash" : "bell"} size={15} />
          {#if muted && level < 2}<span>알림 꺼짐</span>{/if}
        </button>
        <!-- 업데이트 확인 — 새 버전이 있으면 아래 알림 줄에 업데이트 단추가, 없으면 "최신 버전입니다" 가 잠깐 뜬다 -->
        <button
          class="pill"
          style:width="29px"
          class:busy={checking}
          disabled={checking}
          onclick={() => checkUpdate(true)}
          use:hint={"업데이트 확인 — GitHub 에 새 버전이 있는지 확인합니다"}
        >
          <Icon name="refresh" size={17} />
        </button>
        {#if full}
          <!-- 마우스를 올려도 폭이 변하지 않게 글자는 설명으로만 보인다 — 중간 화면은 이미 카드가 접힌 한 줄이라 숨긴다 -->
          <button
            class="pill"
            style:width="31px"
            onclick={foldAll}
            use:hint={allFolded ? "모두 펼치기 — 경로 카드를 펴고 사용 한도를 자세히" : "모두 접기 — 경로 카드를 접고 사용 한도를 한 줄로"}
          >
            <Icon name={allFolded ? "expand" : "compress"} size={17} />
          </button>
        {/if}
        <button class="pin" class:on={pinned} onclick={() => setPinned(!pinned)} use:hint={pinned ? "항상 위에 표시 끄기" : "항상 위에 표시"}>
          <Icon name={pinned ? "pin" : "pin-slash"} size={14} />
          {#if level === 0}<span>{pinned ? "위에 고정" : "고정 안 함"}</span>{/if}
        </button>
      </div>
    {/snippet}
  </Fit>
</div>

<style>
  .header {
    display: flex;
    flex-direction: column;
    gap: 11px;
    padding-top: 7px;
  }
  /* 제목이 먼저 자리를 갖고(macOS layoutPriority(1)) 캐릭터 말풍선이 남는 폭으로 줄어든다 */
  .top {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }
  .title {
    flex: none;
    max-width: 100%;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .name {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }
  .name img {
    flex: none;
    width: 24px;
    height: 24px;
    position: relative;
    top: 2px;
  }
  /* 줄이면 소수점 반올림으로 다 들어가도 말줄임표가 붙는다 — 제목은 줄이지 않는다(가장 좁은 360 에서도 들어간다) */
  .app {
    flex: none;
    font-size: 19px;
    font-weight: 700;
    letter-spacing: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .ver {
    flex: none;
    margin-left: 1px;
    padding: 2px 6px;
    border-radius: 999px;
    font-family: var(--mono);
    font-size: 12px;
    font-weight: 600;
    color: var(--fg2);
    background: rgb(255 255 255 / 0.08);
  }
  .sub {
    display: flex;
    min-width: 0;
    font-size: 12px;
    color: var(--fg2);
    white-space: nowrap;
  }
  .cc {
    color: var(--fg3);
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .mascot {
    /* macOS 는 제목 칸과 말풍선 사이가 8 더 벌어져 말풍선 글이 먼저 줄어든다 */
    padding-left: 8px;
    flex: 0 1000 auto;
    min-width: 0;
    display: flex;
    justify-content: flex-end;
  }
  .header :global(.controlsFit) {
    width: 100%;
  }
  .controls {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
  }
  .chips {
    display: flex;
    gap: 6px;
  }
  .spacer {
    flex: 1;
    min-width: 6px;
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 4px 10px 4px 9px;
    border-radius: 999px;
    font-size: 12px;
    font-weight: 700;
    background: rgb(var(--c) / 0.15);
    transition:
      background 0.15s,
      box-shadow 0.15s;
  }
  .chip:hover {
    background: rgb(var(--c) / 0.25);
  }
  .chip.filtering {
    background: rgb(var(--c) / 0.35);
    box-shadow: inset 0 0 0 1px rgb(var(--c) / 0.9);
  }
  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: rgb(var(--c));
  }
  .pill {
    flex: none;
    min-width: 28px;
    height: 24px;
    padding: 0 8px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 4px;
    border-radius: 12px;
    font-size: 11px;
    font-weight: 600;
    white-space: nowrap;
    color: var(--fg2);
    background: rgb(255 255 255 / 0.07);
    transition:
      background 0.15s,
      color 0.15s;
  }
  .pill:hover:not(:disabled) {
    color: var(--fg);
    background: rgb(255 255 255 / 0.12);
  }
  .pill.busy :global(svg) {
    opacity: 0.4;
  }
  .pill.muted {
    color: rgb(255 115 115);
    background: rgb(255 115 115 / 0.16);
    box-shadow: inset 0 0 0 0.5px rgb(255 115 115 / 0.4);
    --icon-cut: rgb(58 34 40);
  }
  .pin {
    flex: none;
    height: 24px;
    padding: 0 10px 0 9px;
    display: inline-flex;
    align-items: center;
    gap: 5px;
    border-radius: 12px;
    font-size: 11px;
    font-weight: 600;
    white-space: nowrap;
    color: var(--fg2);
    background: rgb(255 255 255 / 0.07);
    transition:
      background 0.2s,
      color 0.2s;
  }
  .pin.on {
    color: rgb(var(--accent));
    background: rgb(var(--accent) / 0.16);
    box-shadow: inset 0 0 0 0.5px rgb(var(--accent) / 0.4);
  }
</style>
