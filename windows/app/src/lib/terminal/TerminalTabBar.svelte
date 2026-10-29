<script lang="ts">
  // 탭 줄(ClaudeBoard.swift TerminalTabBar) — 높이 40, 지금 경로의 탭만. 탭은 120~250 폭으로 나눠 갖고, 120 씩도 안 들어가면 가로로 민다.
  // + 단추는 밀리는 칸 밖이라 늘 보이고 그 오른쪽은 비어 있다
  import { tick } from "svelte";
  import Icon from "$lib/board/Icon.svelte";
  import { hint } from "$lib/board/hint.svelte";
  import { keyLabel } from "$lib/shortcuts/store.svelte";
  import TerminalTabButton from "./TerminalTabButton.svelte";
  import { TerminalHub, newSessionSheet, tabsIn } from "./hub.svelte";
  import { badgeNumber } from "./tabLogic";
  import { scrollTargetFor, tabStripLeast, tabStripWidth } from "./paneLayout";
  import { wheelStep } from "./stripScroll";

  let barWidth = $state(0);
  let newWidth = $state(0); // + 단추 폭(앞 여백 포함) — 탭 칸에 줄 폭을 셀 때
  let scroller = $state<HTMLDivElement>();

  const tabs = $derived(tabsIn(TerminalHub.workspace));
  const count = $derived(tabs.length);
  // 탭이 모두 들어가면 탭마다 가장 넓게, 아니면 남는 폭 전부. 가장 좁게 해도 넘치면 안쪽 폭이 더 커서 가로로 밀린다
  const shown = $derived(tabStripWidth(barWidth, newWidth, count));
  const least = $derived(tabStripLeast(count));
  // 새 세션 키 안내 — 설정 > 단축키에서 바꾼 키를 따른다(비웠으면 안 보인다)
  const newKey = $derived(keyLabel("newSession"));

  // 고른 탭이 칸 밖이면 보이게 민다(가장 가까운 만큼만)
  function scrollToSelected(animated: boolean) {
    const box = scroller;
    const el = box?.querySelector<HTMLElement>(`[data-tab="${TerminalHub.selected}"]`);
    if (!box || !el) return;
    const left = el.offsetLeft;
    const right = left + el.offsetWidth;
    const target = scrollTargetFor(left, right, box.scrollLeft, box.clientWidth);
    if (target !== box.scrollLeft) box.scrollTo({ left: target, behavior: animated ? "smooth" : "auto" });
  }

  // 처음 나타날 때·고른 탭이 바뀔 때(번호 키·새 탭 포함)·칸 폭이 바뀔 때 — 배치가 끝난 뒤에 민다
  let first = true;
  $effect(() => {
    void TerminalHub.selected;
    void count;
    const animated = !first;
    first = false;
    void tick().then(() => requestAnimationFrame(() => scrollToSelected(animated)));
  });
  $effect(() => {
    void shown;
    void tick().then(() => scrollToSelected(false));
  });

  // 마우스 휠로도 가로로 민다
  function wheel(e: WheelEvent) {
    if (!scroller) return;
    const step = wheelStep(e.deltaX, e.deltaY, e.shiftKey);
    if (step.prevent) e.preventDefault();
    scroller.scrollLeft += step.add;
  }
</script>

<div class="bar" bind:clientWidth={barWidth}>
  <div class="scroller" bind:this={scroller} style:width="{shown}px" onwheel={wheel}>
    <div class="tabs" role="tablist" style:width="{Math.max(shown, least)}px">
      {#each tabs as tab, i (tab.id)}
        <TerminalTabButton {tab} selected={tab.id === TerminalHub.selected} number={badgeNumber(TerminalHub.commandHeld, i)} />
      {/each}
    </div>
  </div>
  <button class="new" bind:offsetWidth={newWidth} onclick={newSessionSheet} use:hint={"새 세션"}>
    <span class="box">
      <svg class="dash" aria-hidden="true"><rect x="0.5" y="0.5" rx="7.5" ry="7.5" /></svg>
      <Icon name="plus" size={10} />
      {#if newKey}<span class="key">{newKey}</span>{/if}
    </span>
  </button>
</div>

<style>
  .bar {
    position: relative;
    flex: none;
    height: 40px;
    display: flex;
    align-items: center;
    padding: 0 8px;
    background: var(--term-bar);
  }
  /* 탭 줄 아래 선 */
  .bar::after {
    content: "";
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    height: 1px;
    background: rgb(255 255 255 / 0.083);
    pointer-events: none;
  }
  .scroller {
    position: relative;
    flex: none;
    height: 100%;
    overflow-x: auto;
    overflow-y: hidden;
    scrollbar-width: none;
  }
  .scroller::-webkit-scrollbar {
    display: none;
  }
  .tabs {
    height: 100%;
    display: flex;
    gap: 2px;
  }
  .new {
    flex: none;
    margin-left: 6px;
    height: 26px;
  }
  .box {
    position: relative;
    height: 26px;
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 0 11px;
    border-radius: 8px;
    font-size: 12px;
    font-weight: 600;
    color: rgb(255 255 255 / 0.8);
    transition:
      background 0.15s,
      color 0.15s;
  }
  .new:hover .box {
    color: #fff;
    background: rgb(255 255 255 / 0.08);
  }
  .key {
    font-size: 11px;
    font-weight: 500;
    opacity: 0.75;
  }
  /* 점선 테두리 — 1px, 3 그리고 2 간격, 안쪽으로 그린다(CSS dashed 는 간격이 달라 SVG 로) */
  .dash {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    overflow: visible;
    pointer-events: none;
  }
  .dash rect {
    width: calc(100% - 1px);
    height: calc(100% - 1px);
    fill: none;
    stroke: rgb(255 255 255 / 0.25);
    stroke-width: 1;
    stroke-dasharray: 3 2;
    stroke-dashoffset: 3.5;
  }
</style>
