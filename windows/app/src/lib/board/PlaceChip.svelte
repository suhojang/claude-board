<script lang="ts">
  // 세션 위치 칩(ClaudeBoard.swift PlaceChip): 어디서 도는지 보여 준다. 칩의 자리(kind)는 terminal/place.ts 의
  // pure pairing module(pairSessions·placeOf)이 정하고, 글자·색·힌트는 placeChip.ts 표가 정한다.
  // 이 컴포넌트는 그 화면을 그리는 일만 한다: 세션 줄(SessionRow)은 hub 의 place(s) 로 자리를 받아 항상 그린다
  import type { SessionPlace } from "$lib/terminal/place";
  import { chipView, type ChipMode } from "./placeChip";
  import { hint } from "./hint.svelte";

  let { place, headless = false, appName = null, mode = null }: { place: SessionPlace; headless?: boolean; appName?: string | null; mode?: ChipMode | null } = $props();

  const view = $derived(chipView(place, headless, appName, mode));
</script>

{#if view}
  <span
    class="chip"
    class:mono={view.mono}
    class:muted={place.kind === "ended"}
    style:--bg={view.bg || undefined}
    style:--fg={view.fg || undefined}
    use:hint={view.tip}>{view.text}</span
  >
{/if}

<style>
  .chip {
    flex: none;
    padding: 1.5px 6px;
    border-radius: 999px;
    font-size: 10px;
    font-weight: 600;
    line-height: 13px;
    white-space: nowrap;
    color: rgb(var(--fg));
    background: rgb(var(--bg));
  }
  .chip.mono {
    font-family: var(--mono);
  }
  .chip.muted {
    color: var(--fg2);
    background: rgb(255 255 255 / 0.08);
  }
</style>
