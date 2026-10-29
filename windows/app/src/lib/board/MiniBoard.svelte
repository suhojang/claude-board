<script lang="ts">
  // 미니 화면(ClaudeBoard.swift MiniBoard) — 캐릭터 줄, 경로마다 한 줄(상태 · 이름 · 지금 하는 일), 아래 줄에 사용 한도와 투명도.
  // 한 줄을 누르면 대표 세션의 터미널 앱을, 두 번 누르면 그 세션의 터미널 창을 앞으로(보드 탭 세션이면 그 탭, 터미널 없는 세션이면 안내)
  import { onDestroy } from "svelte";
  import { flip } from "svelte/animate";
  import type { Group, Snapshot } from "$lib/types";
  import Foreman from "$lib/mascot/Foreman.svelte";
  import { setMascot } from "$lib/backend";
  import { createRowClick, type RowClick } from "$lib/terminal/sessionFlow.svelte";
  import CompactLimit from "./CompactLimit.svelte";
  import Fit from "./Fit.svelte";
  import KindLabel from "./KindLabel.svelte";
  import OpacitySlider from "./OpacitySlider.svelte";
  import StateIcon from "./StateIcon.svelte";
  import { active, clock, doing, elapsed, stateName, stateRgb, urgent } from "./format";
  import { hint } from "./hint.svelte";

  let { snap }: { snap: Snapshot } = $props();

  const limits = $derived(snap.usage.limits);

  function rowInfo(g: Group) {
    const busy = active(g);
    const top = busy[0];
    const first = g.sessions[0];
    const live = top ?? g.sessions.find((s) => s.state !== "ended");
    return { busy, top, first, live };
  }

  // 경로 줄마다 클릭 처리기 하나 — 다른 터미널의 한 번 클릭을 미루는 타이머가 줄마다 따로 돈다
  const clickers = new Map<string, RowClick>();
  function clickerOf(g: Group): RowClick {
    let clicker = clickers.get(g.id);
    if (!clicker) {
      const group = g.id;
      clicker = createRowClick(() => {
        const current = snap.groups.find((x) => x.id === group);
        return current && rowInfo(current).live;
      });
      clickers.set(g.id, clicker);
    }
    return clicker;
  }
  // 사라진 경로 줄의 처리기는 타이머째 치운다
  $effect(() => {
    const live = new Set(snap.groups.map((g) => g.id));
    for (const [id, clicker] of clickers) {
      if (live.has(id)) continue;
      clicker.dispose();
      clickers.delete(id);
    }
  });
  onDestroy(() => clickers.forEach((clicker) => clicker.dispose()));
</script>

<div class="mini">
  <!-- 캐릭터와 말풍선 — 최소 크기에서도 보인다 -->
  <div class="mascot">
    <Foreman groups={snap.groups} limits={limits} now={snap.now} compact mascot={snap.settings.mascot} onPickMascot={setMascot} />
  </div>
  {#if snap.groups.length === 0}
    <div class="empty">아직 기록된 세션이 없습니다</div>
  {:else}
    <div class="list">
      {#each snap.groups as g (g.id)}
        {@const r = rowInfo(g)}
        {@const u = urgent(g)}
        <div
          class="row"
          role="button"
          tabindex="-1"
          animate:flip={{ duration: 400 }}
          onclick={(e) => clickerOf(g).click(e)}
          onkeydown={() => {}}
          use:hint={r.live ? `${g.path} · 눌러서 터미널 앱, 두 번 눌러서 그 세션으로 이동` : g.path}
        >
          <StateIcon state={u} scale={0.8} />
          <span class="name">{g.name}</span>
          <span class="what">
            <!-- 줄에 보이는 세션이 터미널에서 도는지 백그라운드로 도는지 — 아이콘만 -->
            {#if r.top ?? r.first}<KindLabel headless={(r.top ?? r.first).headless} compact />{/if}
            {#if r.top}
              <span class="text" style:color="rgb({stateRgb(r.top.state)})">{doing(r.top)}</span>
              {#if r.busy.length > 1}<span class="more" style:color="rgb({stateRgb(r.top.state)})">+{r.busy.length - 1}</span>{/if}
            {:else if r.first}
              <span class="text dim">
                {stateName(r.first.state)} · {r.first.state === "idle"
                  ? `${elapsed(r.first.since, snap.now)} 전`
                  : r.first.state === "ended"
                    ? clock(r.first.updated)
                    : r.first.title}
              </span>
            {/if}
          </span>
          {#if u === "working"}<span class="shimmer" style:--c={stateRgb("working")}></span>{/if}
        </div>
      {/each}
    </div>
  {/if}
  <div class="bottom">
    <!-- 자리가 모자라면 한도를 앞의 둘만, 더 모자라면 빼고 — 투명도 슬라이더는 늘 보인다 -->
    <Fit count={3} class="limits">
      {#snippet content(level)}
        <span class="lims">
          {#each level === 0 ? limits : level === 1 ? limits.slice(0, 2) : [] as l, i (l.id)}
            {#if i > 0}<span class="sep"></span>{/if}
            <CompactLimit limit={l} level={2} now={snap.now} />
          {/each}
        </span>
      {/snippet}
    </Fit>
    <OpacitySlider value={snap.settings.opacity} compact />
  </div>
</div>

<style>
  .mini {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 2px 10px 6px;
  }
  .mascot {
    display: flex;
    justify-content: flex-end;
    flex: none;
  }
  .empty {
    flex: 1;
    display: grid;
    place-items: center;
    font-size: 11px;
    color: var(--fg2);
  }
  .list {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 2px;
    scrollbar-width: none;
  }
  .list::-webkit-scrollbar {
    display: none;
  }
  .row {
    position: relative;
    flex: none;
    display: flex;
    align-items: center;
    gap: 8px;
    height: 24px;
    padding: 0 8px;
    border-radius: 7px;
    cursor: default;
    outline: none;
    transition: background 0.15s;
  }
  .row:hover {
    background: rgb(255 255 255 / 0.07);
  }
  .name {
    flex: none;
    max-width: 50%;
    font-size: 12px;
    font-weight: 600;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .what {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 4px;
    font-size: 11px;
  }
  .text {
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .dim {
    color: var(--fg2);
  }
  .more {
    flex: none;
  }
  .bottom {
    flex: none;
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .bottom :global(.limits) {
    flex: 1;
  }
  .lims {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .sep {
    width: 0.5px;
    height: 10px;
    background: rgb(255 255 255 / 0.14);
  }
</style>
