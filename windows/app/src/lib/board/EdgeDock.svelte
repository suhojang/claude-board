<script lang="ts">
  // 아래쪽 터미널 독(ClaudeBoard.swift EdgeDock, SPEC §5.11) — 평소엔 보드 칸 바깥쪽 끝(보드가 오른쪽이면 오른쪽, 왼쪽이면 왼쪽) 아래의 탭만 보이다가
  // 그 탭에 마우스를 대면 보드 위로 미끄러져 나오고, 독에서 벗어나면 조금 뒤 다시 숨는다. 왼쪽일 때는 좌우를 뒤집어 그린다
  import { fly } from "svelte/transition";
  import type { BoardSide, TerminalApp } from "$lib/types";
  import { openTerminal } from "$lib/backend";
  import Icon from "./Icon.svelte";
  import TerminalIcon from "./TerminalIcon.svelte";
  import { DOCK, pointer } from "./pointer.svelte";

  let { terminals, side = "right" }: { terminals: TerminalApp[]; side?: BoardSide } = $props();
  // 바깥쪽(화면 가장자리 쪽)으로 미끄러지는 방향
  const out = $derived(side === "right" ? 1 : -1);

  let open = $state(false);
  let overPanel = $state(false);
  let hovered = $state<number | null>(null);
  let closing: ReturnType<typeof setTimeout> | undefined;

  // 오른쪽 끝이나 독 위에 있으면 연다. 벗어나면 조금 기다렸다 닫는다 — 끝에서 독으로 옮겨 가는 사이에 닫히지 않게
  $effect(() => {
    const want = pointer.atEdge || overPanel;
    clearTimeout(closing);
    if (want) open = true;
    else if (open) {
      closing = setTimeout(() => {
        open = false;
        hovered = null;
      }, 350);
    }
  });

  const hintTop = (i: number) => DOCK.pad + i * (DOCK.size + DOCK.gap) + (DOCK.size - 26) / 2;
</script>

<div class="dock" class:left={side === "left"} style:bottom="{DOCK.bottomInset}px">
  {#if open}
    <div
      class="panel"
      role="toolbar"
      tabindex="-1"
      onmouseenter={() => (overPanel = true)}
      onmouseleave={() => {
        overPanel = false;
        hovered = null;
      }}
      transition:fly={{ x: 60 * out, duration: 280 }}
    >
      {#each terminals as t, i (t.id)}
        <button
          class="app"
          class:installed={t.installed}
          class:hover={hovered === i}
          onmouseenter={() => (hovered = i)}
          onclick={() => t.installed && openTerminal(t.id)}
          aria-label={t.name}
        >
          <TerminalIcon id={t.id} />
          <!-- 실행 중이면 바깥쪽(오른쪽)에 점 -->
          <span class="run" class:on={t.running}></span>
        </button>
      {/each}
      {#if hovered != null && terminals[hovered]}
        {@const t = terminals[hovered]}
        <!-- 힌트는 독 왼쪽에 — 아이콘을 옮겨 다니면 사라졌다 나타나지 않고 그 아이콘 높이로 미끄러진다 -->
        <div class="tip" class:off={!t.installed} style:top="{hintTop(hovered)}px">
          <Icon name={t.installed ? (t.running ? "app-forward" : "app-plus") : "nosign"} size={12} />
          {t.installed ? (t.running ? `${t.name} 창으로 이동` : `${t.name} 실행`) : `${t.name} · 설치되지 않음`}
        </div>
      {/if}
    </div>
  {:else}
    <!-- 숨어 있을 때의 탭 — 보드에 마우스가 들어오면 밝아지며 끝까지 튀어나온다. 열고 닫기는 창 전체 추적이 맡는다 -->
    <div class="tab" class:lit={pointer.inside} transition:fly={{ x: 16 * out, duration: 220 }}>
      <Icon name={side === "right" ? "chevron-left" : "chevron-right"} size={11.5} />
      <Icon name="terminal" size={14.5} />
    </div>
  {/if}
</div>

<style>
  /* 탭과 독이 바뀌는 동안 둘 다 떠 있으므로 서로 밀지 않게 오른쪽 아래 모서리에 겹쳐 둔다 */
  .dock {
    position: absolute;
    right: 0;
    width: 0;
    height: 0;
    z-index: 50;
  }
  .panel {
    position: absolute;
    right: 8px;
    bottom: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 8px;
    border-radius: 16px;
    background: rgb(28 28 43 / 0.92);
    backdrop-filter: blur(20px);
    box-shadow:
      inset 0 0 0 0.5px rgb(255 255 255 / 0.14),
      -3px 4px 14px rgb(0 0 0 / 0.45);
    pointer-events: auto;
  }
  .app {
    position: relative;
    width: 36px;
    height: 36px;
    display: grid;
    place-items: center;
    filter: grayscale(1);
    opacity: 0.35;
    cursor: default;
    transition: transform 0.25s cubic-bezier(0.3, 1.6, 0.5, 1);
  }
  .app.installed {
    filter: none;
    opacity: 1;
    cursor: pointer;
  }
  .app.installed.hover {
    transform: scale(1.12);
  }
  .run {
    position: absolute;
    right: -8px;
    top: 16px;
    width: 4px;
    height: 4px;
    border-radius: 50%;
  }
  .run.on {
    background: rgb(255 255 255 / 0.8);
  }
  .tip {
    position: absolute;
    right: calc(100% + 8px);
    height: 26px;
    display: flex;
    align-items: center;
    gap: 5px;
    padding: 0 10px;
    white-space: nowrap;
    font-size: 12px;
    font-weight: 600;
    color: rgb(255 255 255 / 0.95);
    background: rgb(26 26 38 / 0.96);
    border-radius: 13px;
    box-shadow:
      inset 0 0 0 0.5px rgb(255 255 255 / 0.14),
      0 2px 8px rgb(0 0 0 / 0.4);
    transition: top 0.26s cubic-bezier(0.3, 1.1, 0.5, 1);
    pointer-events: none;
  }
  .tip.off {
    color: rgb(255 255 255 / 0.55);
  }
  .tab {
    position: absolute;
    right: 0;
    bottom: 0;
    width: 17px;
    height: 58px;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 3.75px;
    color: rgb(255 255 255 / 0.6);
    background: rgb(var(--accent) / 0.25);
    border-radius: 8px 0 0 8px;
    box-shadow: inset 0 0 0 0.5px rgb(255 255 255 / 0.18);
    transform: translateX(4px);
    transition:
      transform 0.2s ease-out,
      background 0.2s ease-out,
      color 0.2s ease-out;
  }
  .tab.lit {
    color: rgb(255 255 255 / 0.95);
    background: rgb(var(--accent) / 0.5);
    transform: none;
  }

  /* 보드가 왼쪽에 있을 때 — 위 배치를 좌우로 뒤집는다 */
  .dock.left {
    right: auto;
    left: 0;
  }
  .left .panel {
    right: auto;
    left: 8px;
    box-shadow:
      inset 0 0 0 0.5px rgb(255 255 255 / 0.14),
      3px 4px 14px rgb(0 0 0 / 0.45);
  }
  .left .run {
    right: auto;
    left: -8px;
  }
  .left .tip {
    right: auto;
    left: calc(100% + 8px);
  }
  .left .tab {
    right: auto;
    left: 0;
    border-radius: 0 8px 8px 0;
    transform: translateX(-4px);
  }
  .left .tab.lit {
    transform: none;
  }
</style>
