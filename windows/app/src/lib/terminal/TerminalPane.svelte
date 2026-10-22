<script lang="ts">
  // 터미널 칸 하나 — 컨트롤러의 host 를 이 칸에 끼우고 크기 변화를 알려 준다.
  // 접혀도 언마운트하지 않고 숨기기만 한다(출력은 계속 쌓인다). 셸이 끝나면 허브가 그 탭을 닫으므로 안내는 없고,
  // 시작하지 못했을 때만(Windows 에만 있는 경우) 안내와 [다시 시도] [탭 닫기] 를 얹는다
  import { onMount } from "svelte";
  import { controllerOf, hub, remove, retry, startLeaf } from "./hub.svelte";

  let { leafId, tabId, visible }: { leafId: string; tabId: string; visible: boolean } = $props();

  const ctl = $derived(controllerOf(leafId));
  const leaf = $derived(hub.leaves[leafId]);
  const tab = $derived(hub.tabs.find((t) => t.id === tabId));
  let box = $state<HTMLDivElement>();

  onMount(() => {
    if (!box) return;
    box.appendChild(ctl.host);
    const observer = new ResizeObserver(() => ctl.notifyBox());
    observer.observe(box);
    return () => {
      observer.disconnect();
      // 칸이 사라져도 컨트롤러와 xterm 은 허브가 쥐고 있어 다른 부모에 다시 끼울 수 있다
      ctl.host.remove();
    };
  });

  $effect(() => {
    ctl.setVisible(visible);
    if (visible) void startLeaf(leafId);
  });
</script>

<div class="pane" bind:this={box}>
  {#if leaf?.phase === "failed"}
    <div class="notice error">
      <span>터미널을 시작하지 못했습니다: {leaf.error}</span>
      <button onclick={() => retry(leafId)}>다시 시도</button>
      <button class="plain" onclick={() => tab && remove(tab)}>탭 닫기</button>
    </div>
  {/if}
</div>

<style>
  .pane {
    position: relative;
    width: 100%;
    height: 100%;
    min-width: 0;
    min-height: 0;
    background: var(--term-bg);
    overflow: hidden;
  }
  /* host 는 컨트롤러가 만들어 JS 로 끼우므로 :global. 안쪽 여백은 .xterm 에 둔다 — fit 은 그 요소의 여백을 빼고 칸 수를 계산한다 */
  .pane :global(.term-host) {
    position: absolute;
    inset: 0;
    user-select: text;
    -webkit-user-select: text;
  }
  .pane :global(.term-host .xterm) {
    padding: 6px 0 4px 10px;
  }
  .notice {
    position: absolute;
    left: 50%;
    bottom: 16px;
    transform: translateX(-50%);
    /* xterm 안쪽 레이어(링크·선택·스크롤바, 최대 10)보다 위여야 마우스로 누를 수 있다 */
    z-index: 20;
    display: flex;
    align-items: center;
    gap: 12px;
    max-width: calc(100% - 32px);
    padding: 8px 8px 8px 14px;
    border-radius: 10px;
    font-size: 12px;
    color: var(--fg);
    background: rgb(40 40 58 / 0.96);
    box-shadow:
      inset 0 0 0 0.5px rgb(255 255 255 / 0.16),
      0 4px 14px rgb(0 0 0 / 0.5);
  }
  .notice.error {
    color: rgb(var(--red));
  }
  .notice button {
    flex: none;
    padding: 5px 12px;
    border-radius: 7px;
    font-size: 12px;
    font-weight: 600;
    color: #fff;
    background: rgb(var(--accent) / 0.85);
    transition: background 0.15s;
  }
  .notice button:hover {
    background: rgb(var(--accent));
  }
  .notice button.plain {
    background: rgb(255 255 255 / 0.1);
  }
  .notice button.plain:hover {
    background: rgb(255 255 255 / 0.16);
  }
</style>
