<script lang="ts">
  // 간략 모드의 한도 하나 — 이름 · 작은 막대 · 비율. 마우스를 올리면 초기화 시각.
  // level 0 이름·막대·비율, 1 이름·비율, 2 짧은 이름·비율
  import type { Limit } from "$lib/types";
  import { resetText, usageRgb } from "./format";
  import { hint } from "./hint.svelte";

  let { limit, level, now }: { limit: Limit; level: number; now: number } = $props();
  const pct = $derived(Math.round(limit.pct));
</script>

<span class="lim" style:--c={usageRgb(limit.pct)} use:hint={`${limit.label} ${pct}% · ${resetText(limit.resets, now)}`}>
  <span class="name">{level === 2 ? limit.tiny : limit.short}</span>
  {#if level === 0}
    <span class="track"><span class="fill" style:width="{Math.max(2, (26 * Math.min(limit.pct, 100)) / 100)}px"></span></span>
  {/if}
  <span class="pct num">{pct}%</span>
</span>

<style>
  .lim {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    flex: none;
    white-space: nowrap;
  }
  .name {
    font-size: 11px;
    font-weight: 600;
    color: var(--fg2);
  }
  .track {
    position: relative;
    width: 26px;
    height: 4px;
    border-radius: 2px;
    background: rgb(255 255 255 / 0.1);
  }
  .fill {
    position: absolute;
    left: 0;
    top: 0;
    bottom: 0;
    border-radius: 2px;
    background: rgb(var(--c));
  }
  .pct {
    font-size: 13px;
    font-weight: 700;
    color: rgb(var(--c));
  }
</style>
