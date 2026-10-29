<script lang="ts">
  // 경로 줄(ClaudeBoard.swift TerminalWorkspaceBar) — 탭이 있는 작업 경로들(탭 목록에 처음 나온 순서). 높이 32.
  // 넘치면 가로로 밀고(이름은 줄이지 않는다) 지금 경로가 늘 보이게 한다. 경로를 닫는 단추는 없다
  import { tick } from "svelte";
  import WorkspaceButton from "./WorkspaceButton.svelte";
  import { scrollTargetClamped, wheelStep } from "./stripScroll";
  import { TerminalHub, tabsIn, workspaceHint, workspaceKey, workspaceLabel, workspaceState, workspaces } from "./hub.svelte";

  let scroller = $state<HTMLDivElement>();
  let width = $state(0);

  const list = $derived(workspaces());
  // 경로 개수만 따로 본다 — list 는 상태 묶음마다(약 1초) 새 배열이라 그 자체를 보면 손으로 민 스크롤을 지금 경로로 되돌려 버린다
  const workspaceCount = $derived(list.length);

  function scrollToCurrent(animated: boolean) {
    const box = scroller;
    if (!box) return;
    const el = [...box.querySelectorAll<HTMLElement>("[data-ws]")].find((x) => workspaceKey(x.dataset.ws ?? "") === workspaceKey(TerminalHub.workspace));
    if (!el) return;
    // 안쪽 좌표 — 줄 양끝 여백(8)까지 보이게, 0..끝 사이로 맞춘다(규칙은 stripScroll.ts)
    const target = scrollTargetClamped(el.offsetLeft, el.offsetLeft + el.offsetWidth, box.scrollLeft, box.clientWidth, box.scrollWidth, 8);
    if (target !== box.scrollLeft) box.scrollTo({ left: target, behavior: animated ? "smooth" : "auto" });
  }

  let first = true;
  $effect(() => {
    void TerminalHub.workspace;
    void workspaceCount;
    const animated = !first;
    first = false;
    void tick().then(() => requestAnimationFrame(() => scrollToCurrent(animated)));
  });
  $effect(() => {
    void width;
    void tick().then(() => scrollToCurrent(false));
  });

  // 휠·Shift+휠은 가로로 민다. 트랙패드 가로 밀기는 브라우저가 한다
  function wheel(e: WheelEvent) {
    if (!scroller) return;
    const step = wheelStep(e.deltaX, e.deltaY, e.shiftKey);
    if (step.prevent) e.preventDefault();
    scroller.scrollLeft += step.add;
  }
</script>

<div class="row" bind:clientWidth={width}>
  <div class="scroller" bind:this={scroller} onwheel={wheel}>
    <div class="items">
      {#each list as ws (workspaceKey(ws))}
        <WorkspaceButton
          path={ws}
          label={workspaceLabel(ws, list)}
          count={tabsIn(ws).length}
          state={workspaceState(ws)}
          current={workspaceKey(ws) === workspaceKey(TerminalHub.workspace)}
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
