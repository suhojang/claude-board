<script lang="ts">
  // 칸 트리를 그린다 — 잎은 터미널 칸, 나누기는 비율대로 두 쪽. 1단계는 잎 하나만 쓰지만 나누기를 넣을 수 있는 모양이다
  import type { PaneNode } from "$lib/types";
  import PaneView from "./PaneView.svelte";
  import TerminalPane from "./TerminalPane.svelte";

  let { node, tabId, visible }: { node: PaneNode; tabId: string; visible: boolean } = $props();
</script>

{#if node.type === "leaf"}
  <TerminalPane leafId={node.leafId} {tabId} {visible} />
{:else}
  <div class="split" class:col={node.dir === "col"}>
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
