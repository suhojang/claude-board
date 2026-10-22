<script lang="ts">
  // 탭이 하나도 없을 때(ClaudeBoard.swift:7347-7366 TerminalEmptyView)
  import Icon from "$lib/board/Icon.svelte";
  import { newSessionSheet } from "./hub.svelte";
</script>

<div class="empty">
  <div class="t1">보드에서 띄운 세션이 없습니다</div>
  <div class="t2">보드에서 시작하거나 이어 연 세션이 여기 탭으로 열립니다</div>
  <button class="new" onclick={newSessionSheet}>
    <Icon name="plus" size={11} />
    <span>새 세션</span>
  </button>
</div>

<style>
  .empty {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    text-align: center;
  }
  .t1 {
    font-size: 14px;
    line-height: 17px;
    position: relative;
    top: -2px;
    font-weight: 600;
  }
  .t2 {
    font-size: 12px;
    line-height: 14px;
    position: relative;
    top: -1px;
    color: var(--fg2);
  }
  .new {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 30px;
    padding: 0 14px;
    border-radius: 999px;
    font-size: 12px;
    font-weight: 600;
    background: rgb(255 255 255 / 0.126);
    transition: background 0.15s;
  }
  .new:hover {
    background: rgb(255 255 255 / 0.2);
  }
</style>
