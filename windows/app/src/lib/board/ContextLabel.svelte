<script lang="ts">
  // 컨텍스트 사용량 라벨: 작은 막대 + 비율 + 토큰 수.
  // level 0 막대·비율·토큰, 1 막대 없이 비율·토큰, 2 비율만 — 줄이 좁을수록 줄인다
  import type { Ctx } from "$lib/types";
  import Icon from "./Icon.svelte";
  import { ctxRgb, grouped, tokens } from "./format";
  import { hint } from "./hint.svelte";

  let { ctx, level = 0 }: { ctx: Ctx; level?: number } = $props();
</script>

<span class="cap num" style:--c={ctxRgb(ctx.pct)} use:hint={`컨텍스트 ${grouped(ctx.used)} / ${grouped(ctx.size)} 토큰`}>
  <Icon name="brain" size={10} pad={2} />
  {#if level === 0}
    <span class="track"><span class="fill" style:width="{Math.max(2, (26 * ctx.pct) / 100)}px"></span></span>
  {/if}
  {level === 2 ? `${ctx.pct}%` : `${ctx.pct}% · ${tokens(ctx.used)}`}
</span>

<style>
  .cap {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    flex: none;
    padding: 1.5px 6px;
    font-size: 10px;
    font-weight: 600;
    line-height: 13px;
    white-space: nowrap;
    color: rgb(var(--c));
    background: rgb(var(--c) / 0.12);
    box-shadow: inset 0 0 0 0.5px rgb(var(--c) / 0.22);
    border-radius: 999px;
  }
  .track {
    position: relative;
    width: 26px;
    height: 4px;
    border-radius: 2px;
    background: rgb(var(--c) / 0.18);
  }
  .fill {
    position: absolute;
    left: 0;
    top: 0;
    bottom: 0;
    border-radius: 2px;
    background: rgb(var(--c));
    transition: width 0.4s ease-in-out;
  }
</style>
