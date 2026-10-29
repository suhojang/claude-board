<script lang="ts">
  // 화면 나누기 트리를 그린다 — 잎은 터미널 화면, 가지는 비율대로 두 쪽. 지금은 잎 하나만 쓰지만 나누기를 넣을 수 있는 모양이다
  import type { SplitNode } from "$lib/types";
  import PaneView from "./PaneView.svelte";
  import TerminalPane from "./TerminalPane.svelte";

  let { node, tabId, visible }: { node: SplitNode; tabId: string; visible: boolean } = $props();
</script>

{#if "leaf" in node}
  <TerminalPane leafId={node.leaf} {tabId} {visible} />
{:else}
  <div class="split" class:col={node.split === "down"}>
    <div class="side" style:flex="{node.ratio} 1 0">
      <PaneView node={node.first} {tabId} {visible} />
    </div>
    <div class="side" style:flex="{1 - node.ratio} 1 0">
      <PaneView node={node.second} {tabId} {visible} />
    </div>
  </div>
{/if}

<style>
  .split {
    display: flex;
    width: 100%;
    height: 100%;
  }
  .split.col {
    flex-direction: column;
  }
  .side {
    min-width: 0;
    min-height: 0;
  }
</style>
