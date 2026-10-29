<script lang="ts">
  // 제목 막대의 "터미널" 단추(ClaudeBoard.swift TerminalToggleButton) — 캡슐. 펼침은 작업 중 색, 접힘은 흰 반투명.
  // 미니 단계에서는 부르는 쪽이 그리지 않는다
  import { setTerminalExpanded } from "$lib/backend";
  import { hint } from "$lib/board/hint.svelte";
  import { keyLabel } from "$lib/shortcuts/store.svelte";
  import type { BoardSide } from "$lib/types";

  let { expanded, side }: { expanded: boolean; side: BoardSide } = $props();

  const left = $derived(side === "left");
  const key = $derived(keyLabel("terminal"));
  const keyNote = $derived(key ? ` (${key})` : "");
  const tip = $derived(
    expanded
      ? `터미널 접기: 보드만 ${left ? "왼쪽" : "오른쪽"}에 남깁니다${keyNote}`
      : `터미널 펼치기: 보드에서 띄운 세션을 ${left ? "오른쪽" : "왼쪽"} 탭에서 봅니다${keyNote}`,
  );
</script>

<button class="toggle" class:on={expanded} onclick={() => setTerminalExpanded(!expanded)} use:hint={tip} aria-pressed={expanded}>
  <span class="cap">
    <!-- SF Symbol rectangle.(leading|trailing)half.inset.filled — 14x11, 속 채움이 반쪽 -->
    <svg class="sym" width="14" height="11" viewBox="0 0 14 11" aria-hidden="true">
      <rect x="0.5" y="0.5" width="13" height="10" rx="2.2" fill="none" stroke="currentColor" stroke-width="1" />
      <rect x={left ? 7.7 : 2.2} y="2.2" width="3.6" height="6.6" rx="0.6" fill="currentColor" />
    </svg>
    <span class="text">{expanded ? "터미널 펼침" : "터미널 접힘"}</span>
    {#if key}<span class="key">{key}</span>{/if}
  </span>
</button>

<style>
  /* 눌리는 칸은 높이 28, 캡슐은 그 안 20 */
  .toggle {
    flex: none;
    height: 28px;
    display: flex;
    align-items: center;
  }
  .cap {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    height: 20px;
    padding: 0 8px;
    border-radius: 999px;
    font-size: 11px;
    font-weight: 600;
    white-space: nowrap;
    color: rgb(255 255 255 / 0.8);
    background: rgb(255 255 255 / 0.18);
    box-shadow: inset 0 0 0 1px rgb(255 255 255 / 0.18);
    transition:
      color 0.15s ease-out,
      background 0.15s ease-out;
  }
  .toggle:hover .cap {
    color: #fff;
    background: rgb(255 255 255 / 0.3);
  }
  /* 펼침 색은 mac 의 반투명 합성이 같은 알파에서 더 진하게 나오는 실측 값에 맞춘 불투명 값이다(바탕은 --term-bar 위) */
  .toggle.on .cap {
    color: rgb(var(--working));
    background: rgb(44 56 83);
    box-shadow: inset 0 0 0 1px rgb(60 70 95);
  }
  .sym {
    flex: none;
  }
  .key {
    padding: 0 4px;
    border-radius: 4px;
    font-size: 10px;
    font-weight: 500;
    line-height: 14px;
    background: rgb(255 255 255 / 0.08);
  }
  .toggle.on .key {
    background: rgb(55 67 91);
  }
</style>
