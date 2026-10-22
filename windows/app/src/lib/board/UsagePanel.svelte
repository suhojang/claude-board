<script lang="ts">
  // 사용 한도 카드(ClaudeBoard.swift:1479-1628): 5시간·주간 전체·주간 모델별 막대. 머리에 로그인 계정(눌러서 전환),
  // 오른쪽 위 단추로 간략(한 줄)·상세를 바꾼다. 중간 화면(forceCompact)에서는 저장한 설정은 두고 한 줄로만 보인다
  import { fade, slide } from "svelte/transition";
  import type { Snapshot } from "$lib/types";
  import { setUsageCompact } from "$lib/backend";
  import AccountChip from "./AccountChip.svelte";
  import CompactLimit from "./CompactLimit.svelte";
  import Fit from "./Fit.svelte";
  import Icon from "./Icon.svelte";
  import { resetText, usageRgb } from "./format";
  import { hint } from "./hint.svelte";

  let { snap, forceCompact = false }: { snap: Snapshot; forceCompact?: boolean } = $props();

  const usage = $derived(snap.usage);
  const accounts = $derived(snap.accounts);
  const setting = $derived(snap.settings.usageCompact);
  const compact = $derived(setting || forceCompact);
  const notice = $derived(accounts.notice);
  const status = $derived(
    usage.tokenExpired
      ? "로그인 토큰 갱신 전 — 세션이 다음 요청을 보내면 보입니다"
      : usage.failed
        ? "사용량을 불러오지 못했습니다"
        : "사용량 불러오는 중…",
  );
</script>

{#snippet modeButton()}
  <button
    class="mode"
    onclick={() => setUsageCompact(!setting)}
    use:hint={setting ? "자세히 보기 — 한도마다 막대와 초기화 시각" : "간략히 보기 — 한 줄로"}
  >
    <Icon name={setting ? "expand" : "compress"} size={15} />
  </button>
{/snippet}

{#snippet statusLine()}
  <span class="status"><Icon name="gauge" size={11} /><span>{status}</span></span>
{/snippet}

<div class="panel" class:compact>
  {#if compact}
    <div class="line">
      <AccountChip {accounts} compact />
      {#if usage.limits.length === 0}
        {@render statusLine()}
      {:else}
        <!-- 넓으면 이름·막대·비율, 좁으면 막대를 빼고, 더 좁으면 이름을 줄인다 -->
        <Fit count={3} class="limits">
          {#snippet content(level)}
            <span class="row">
              {#each usage.limits as l, i (l.id)}
                {#if i > 0}<span class="sep"></span>{/if}
                <CompactLimit limit={l} {level} now={snap.now} />
              {/each}
            </span>
          {/snippet}
        </Fit>
      {/if}
      <!-- 중간 화면에서는 바꿔도 보이지 않으므로 숨긴다 -->
      {#if !forceCompact}{@render modeButton()}{/if}
    </div>
  {:else}
    <div class="line">
      <AccountChip {accounts} />
      <span class="spacer"></span>
      {@render modeButton()}
    </div>
    {#if usage.limits.length === 0}{@render statusLine()}{/if}
    {#each usage.limits as l (l.id)}
      {@const pct = Math.round(l.pct)}
      <div class="limit" style:--c={usageRgb(l.pct)} transition:fade={{ duration: 200 }}>
        <div class="head">
          <span class="label">{l.label}</span>
          <span class="reset num">{resetText(l.resets, snap.now)}</span>
          <span class="pct num">{pct}%</span>
        </div>
        <div class="bar">
          <span class="fill" style:width="max(4px, {Math.min(l.pct, 100)}%)"></span>
        </div>
      </div>
    {/each}
  {/if}
  {#if notice}
    <div class="notice" class:error={notice.error} transition:slide={{ duration: 200 }}>
      <Icon name={notice.error ? "warn" : "check-circle"} size={11} />
      <span>{notice.text}</span>
    </div>
  {/if}
</div>

<style>
  .panel {
    display: flex;
    flex-direction: column;
    gap: 9px;
    padding: 12px;
    border-radius: 14px;
    background: rgb(255 255 255 / 0.05);
    box-shadow: inset 0 0 0 0.5px rgb(255 255 255 / 0.08);
  }
  .panel.compact {
    gap: 0;
    padding: 6px 8px;
  }
  .line {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }
  .spacer {
    flex: 1;
    min-width: 0;
  }
  /* 남는 폭을 다 받아야 넓어졌을 때 자세한 단계로 돌아간다 */
  .line :global(.limits) {
    flex: 1 1 0;
  }
  .row {
    display: inline-flex;
    align-items: center;
    gap: 8px;
  }
  .sep {
    width: 0.5px;
    height: 12px;
    background: rgb(255 255 255 / 0.14);
  }
  .mode {
    flex: none;
    width: 24px;
    height: 20px;
    border-radius: 10px;
    display: grid;
    place-items: center;
    color: var(--fg2);
    background: rgb(255 255 255 / 0.06);
    transition: background 0.15s;
  }
  .mode:hover {
    color: var(--fg);
    background: rgb(255 255 255 / 0.14);
  }
  .status {
    flex: 1;
    display: inline-flex;
    align-items: center;
    gap: 5px;
    min-width: 0;
    font-size: 11px;
    color: var(--fg2);
    white-space: nowrap;
  }
  .status span {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .limit {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .head {
    display: flex;
    align-items: baseline;
    gap: 6px;
  }
  .label {
    flex: 1;
    min-width: 0;
    font-size: 12px;
    font-weight: 600;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .reset {
    font-size: 10.5px;
    color: var(--fg2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .pct {
    min-width: 38px;
    text-align: right;
    font-size: 13px;
    font-weight: 700;
    color: rgb(var(--c));
  }
  .bar {
    position: relative;
    height: 6px;
    border-radius: 3px;
    background: rgb(255 255 255 / 0.08);
  }
  .fill {
    position: absolute;
    left: 0;
    top: 0;
    bottom: 0;
    border-radius: 3px;
    background: linear-gradient(90deg, rgb(var(--c) / 0.65), rgb(var(--c)));
    box-shadow: 0 0 4px rgb(var(--c) / 0.45);
    transition: width 0.6s cubic-bezier(0.3, 1.1, 0.5, 1);
  }
  .notice {
    display: flex;
    align-items: flex-start;
    gap: 5px;
    font-size: 10.5px;
    font-weight: 500;
    color: rgb(107 217 153);
  }
  .compact .notice {
    padding-top: 6px;
  }
  .notice.error {
    color: rgb(255 148 115);
  }
  .notice :global(svg) {
    margin-top: 1px;
  }
  .notice span {
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }
</style>
