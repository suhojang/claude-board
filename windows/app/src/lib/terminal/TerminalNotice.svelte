<script lang="ts">
  // 보드 안 알림(mac TerminalNotice) — 터미널이 없는 세션을 눌렀을 때처럼 한 줄 안내를 6초 동안 아래에 띄운다. 누르면 바로 닫는다
  import { fly } from "svelte/transition";
  import { TerminalHub } from "./hub.svelte";
  import { dismissNotice } from "./sessionFlow.svelte";
</script>

{#if TerminalHub.notice}
  <div class="notice" role="status" transition:fly={{ y: 12, duration: 180 }}>
    <button class="text" onclick={dismissNotice}>{TerminalHub.notice}</button>
  </div>
{/if}

<style>
  .notice {
    position: fixed;
    left: 12px;
    right: 12px;
    bottom: 14px;
    z-index: 950;
    display: flex;
    justify-content: center;
    pointer-events: none;
  }
  .text {
    pointer-events: auto;
    max-width: 480px;
    padding: 9px 14px;
    border-radius: 10px;
    font-size: 12px;
    line-height: 1.45;
    text-align: left;
    color: rgb(255 255 255 / 0.95);
    background: rgb(30 30 34 / 0.96);
    box-shadow:
      inset 0 0 0 0.5px rgb(255 255 255 / 0.18),
      0 6px 20px rgb(0 0 0 / 0.45);
  }
</style>
