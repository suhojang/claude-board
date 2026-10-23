<script lang="ts">
  // 경로 카드 안 구역 머리글 — 접기 화살표·종류 아이콘·이름, 상태별 색 점·개수(있는 상태만)와 가는 선. 누르면 구역을 접고 편다
  import type { Session } from "$lib/types";
  import Icon from "./Icon.svelte";
  import { stateName, stateRgb } from "./format";
  import { hint } from "./hint.svelte";

  let {
    headless,
    sessions,
    collapsed,
    toggle,
  }: { headless: boolean; sessions: Session[]; collapsed: boolean; toggle: () => void } = $props();

  const counts = $derived(
    (["waiting", "working", "paused", "idle"] as const)
      .map((st) => ({ st, n: sessions.filter((s) => s.state === st).length }))
      .filter((x) => x.n > 0),
  );
</script>

<div
  class="kind"
  role="button"
  tabindex="-1"
  style:--k={headless ? "184 140 255" : "82 199 219"}
  onclick={toggle}
  onkeydown={(e) => e.key === "Enter" && toggle()}
  use:hint={(headless
    ? "터미널 없이 도는 세션 — 더블 클릭으로 이동할 터미널이 없습니다"
    : "터미널 창에서 도는 세션 — 더블 클릭하면 그 터미널로 이동합니다") + (collapsed ? " (누르면 펼치기)" : " (누르면 접기)")}
>
  <span class="chev" class:open={!collapsed}><Icon name="chevron-right" size={8} /></span>
  <Icon name={headless ? "moon-stars" : "terminal"} size={12.5} />
  <span class="name">{headless ? "백그라운드" : "터미널"}</span>
  {#each counts as c (c.st)}
    <span class="count num" use:hint={`${stateName(c.st)} ${c.n}개`}>
      <span class="dot" style:background="rgb({stateRgb(c.st)})"></span>{c.n}
    </span>
  {/each}
  <span class="line"></span>
</div>

<style>
  .kind {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-top: 1px;
    padding: 3px 10px;
    border-radius: 7px;
    color: rgb(var(--k) / 0.85);
    cursor: pointer;
    outline: none;
    transition: background 0.15s;
  }
  .kind:hover {
    color: rgb(var(--k));
    background: rgb(255 255 255 / 0.05);
  }
  .chev {
    display: grid;
    place-items: center;
    width: 8px;
  }
  .chev :global(svg) {
    transition: transform 0.3s cubic-bezier(0.3, 1.2, 0.5, 1);
  }
  .chev.open :global(svg) {
    transform: rotate(90deg);
  }
  .name {
    font-size: 10.5px;
    font-weight: 600;
  }
  .count {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    font-size: 10px;
    font-weight: 600;
    color: var(--fg2);
  }
  .dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
  }
  .line {
    flex: 1;
    height: 0.5px;
    min-height: 0.5px;
    background: rgb(var(--k) / 0.22);
  }
</style>
