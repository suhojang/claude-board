<script lang="ts">
  // 경로 줄(ClaudeBoard.swift:6859-6893 TerminalWorkspaceBar) — 탭이 있는 작업 경로들(탭 목록에 처음 나온 순서). 높이 32.
  // 넘치면 가로로 밀고(이름은 줄이지 않는다) 지금 경로가 늘 보이게 한다. 경로를 닫는 단추는 없다
  import { tick } from "svelte";
  import PathItem from "./PathItem.svelte";
  import { hub, tabsIn, workspaceHint, workspaceKey, workspaceLabel, workspaceState, workspaces } from "./hub.svelte";

  let scroller = $state<HTMLDivElement>();
  let width = $state(0);

  const list = $derived(workspaces());

  function scrollToCurrent(animated: boolean) {
    const box = scroller;
    if (!box) return;
    const el = [...box.querySelectorAll<HTMLElement>("[data-ws]")].find((x) => workspaceKey(x.dataset.ws ?? "") === workspaceKey(hub.workspace));
    if (!el) return;
    const left = el.offsetLeft - 8;
    const right = el.offsetLeft + el.offsetWidth + 8;
    let target = box.scrollLeft;
    if (left < box.scrollLeft) target = Math.max(0, left);
    else if (right > box.scrollLeft + box.clientWidth) target = right - box.clientWidth;
    if (target !== box.scrollLeft) box.scrollTo({ left: target, behavior: animated ? "smooth" : "auto" });
  }

  let first = true;
  $effect(() => {
    void hub.workspace;
    void list.length;
    const animated = !first;
    first = false;
    void tick().then(() => requestAnimationFrame(() => scrollToCurrent(animated)));
  });
  $effect(() => {
    void width;
    void tick().then(() => scrollToCurrent(false));
  });

  function wheel(e: WheelEvent) {
    if (!scroller || e.deltaX !== 0) return;
    scroller.scrollLeft += e.deltaY;
  }
</script>

<div class="row" bind:clientWidth={width}>
  <div class="scroller" bind:this={scroller} onwheel={wheel}>
    <div class="items">
      {#each list as ws (workspaceKey(ws))}
        <PathItem
          path={ws}
          label={workspaceLabel(ws, list)}
          count={tabsIn(ws).length}
          state={workspaceState(ws)}
          current={workspaceKey(ws) === workspaceKey(hub.workspace)}
          tip={workspaceHint(ws)}
        />
      {/each}
    </div>
  </div>
</div>

<style>
  .row {
    flex: none;
    height: 32px;
    background: var(--term-bar);
  }
  .scroller {
    position: relative;
    height: 100%;
    padding: 0 8px;
    overflow-x: auto;
    overflow-y: hidden;
    scrollbar-width: none;
  }
  .scroller::-webkit-scrollbar {
    display: none;
  }
  /* 다 들어가면 왼쪽부터 늘어서고, 넘치면 안쪽이 길어져 가로로 밀린다 */
  .items {
    height: 100%;
    width: max-content;
    min-width: 100%;
    display: flex;
    align-items: center;
    gap: 4px;
  }
</style>
