<script lang="ts">
  // 경로 줄 항목(ClaudeBoard.swift:6896-6929 WorkspaceButton) — 상태 점 · 폴더 이름 · 탭 수. 지금 경로는 고른 탭처럼 보인다
  import { hint } from "$lib/board/hint.svelte";
  import { stateRgb } from "$lib/board/format";
  import { showWorkspace } from "./hub.svelte";
  import type { State } from "$lib/types";

  let { path, label, count, state, current, tip }: { path: string; label: string; count: number; state: State; current: boolean; tip: string } = $props();
</script>

<button
  class="item"
  class:current
  data-ws={path}
  onpointerdown={(e) => e.preventDefault()}
  onclick={() => showWorkspace(path)}
  use:hint={tip}
>
  <span class="dot" style:background="rgb({stateRgb(state)})"></span>
  <span class="label">{label}</span>
  <span class="count num">{count}</span>
</button>

<style>
  .item {
    flex: none;
    height: 24px;
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 0 10px;
    border-radius: 7px;
    color: var(--fg2);
    cursor: default;
  }
  .item:hover {
    background: rgb(255 255 255 / 0.06);
  }
  .item.current {
    color: var(--fg);
    background: var(--term-bg);
    box-shadow: inset 0 0 0 1px rgb(var(--working) / 0.5);
  }
  .dot {
    flex: none;
    width: 7px;
    height: 7px;
    border-radius: 50%;
  }
  .label {
    font-size: 12px;
    font-weight: 600;
    white-space: nowrap;
  }
  .count {
    font-size: 11px;
    font-weight: 600;
    color: var(--fg2);
  }
</style>
