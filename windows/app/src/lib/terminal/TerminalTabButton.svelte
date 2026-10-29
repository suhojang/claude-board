<script lang="ts">
  // 탭 하나(ClaudeBoard.swift BoardTabButton) — 높이 40. 점(상태) · 이름 · 하는 일 · 닫기.
  // 수정키(Ctrl+Alt)를 누르는 동안은 × 자리에 탭 번호, 고른 탭의 하는 일 자리에 이름 바꾸기 키를 보인다
  import Icon from "$lib/board/Icon.svelte";
  import { hint } from "$lib/board/hint.svelte";
  import { stateRgb } from "$lib/board/format";
  import { keyLabel } from "$lib/shortcuts/store.svelte";
  import type { BoardTab } from "$lib/types";
  import TabNameField from "./TabNameField.svelte";
  import { doing, leaveSettings, TerminalHub, requestClose, select, shownName, tabState } from "./hub.svelte";

  let { tab, selected, number }: { tab: BoardTab; selected: boolean; number: number | null } = $props();
  const renameKey = $derived(keyLabel("rename"));
</script>

<!-- 탭은 포커스를 터미널에 두므로 키보드는 단축키(Ctrl+Alt+1~9·좌우)가 맡는다 -->
<!-- svelte-ignore a11y_click_events_have_key_events -->
<div
  class="tab"
  class:selected
  role="tab"
  tabindex="-1"
  aria-selected={selected}
  data-tab={tab.id}
  onpointerdown={(e) => {
    // 이름 칸을 누를 때는 입력 칸이 포커스를 가져야 하고, 그 밖은 포커스를 터미널에 둔 채 고른다
    if (!tab.renaming && e.button === 0) e.preventDefault();
  }}
  onclick={() => {
    if (tab.renaming) return;
    leaveSettings();
    select(tab.id);
  }}
>
  <span class="dot" style:background="rgb({stateRgb(tabState(tab))})"></span>
  {#if tab.renaming}
    <TabNameField {tab} />
  {:else}
    <span class="name">{shownName(tab)}</span>
  {/if}
  {#if TerminalHub.commandHeld && selected}
    <span class="doing">{renameKey ? `${renameKey} ` : ""}이름 바꾸기</span>
  {:else}
    <span class="doing">{doing(tab)}</span>
  {/if}
  {#if number !== null}
    <span class="badge num">{number}</span>
  {:else}
    <button
      class="close"
      onclick={(e) => {
        e.stopPropagation();
        requestClose(tab);
      }}
      use:hint={"탭 닫기"}
      aria-label="탭 닫기"
    >
      <Icon name="xmark" size={8} />
    </button>
  {/if}
</div>

<style>
  .tab {
    position: relative;
    flex: 1 1 0;
    min-width: 120px;
    max-width: 250px;
    height: 100%;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 8px 0 14px;
    overflow: hidden;
    color: var(--fg2);
    cursor: default;
  }
  .tab:hover {
    background: rgb(255 255 255 / 0.04);
  }
  .tab.selected {
    color: var(--fg);
    background: var(--term-bg);
  }
  /* 고른 탭의 위쪽 줄 */
  .tab.selected::before {
    content: "";
    position: absolute;
    left: 0;
    right: 0;
    top: 0;
    height: 2px;
    background: rgb(var(--working));
  }
  .dot {
    flex: none;
    width: 8px;
    height: 8px;
    border-radius: 50%;
  }
  .name {
    flex: none;
    font-size: 13px;
    font-weight: 600;
    white-space: nowrap;
  }
  .doing {
    flex: 1 1 0;
    min-width: 0;
    font-size: 12px;
    color: var(--fg2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .tab.selected .doing {
    color: var(--fg2);
  }
  .close,
  .badge {
    flex: none;
    width: 16px;
    height: 16px;
    display: grid;
    place-items: center;
    border-radius: 50%;
    color: var(--fg2);
    opacity: 0;
  }
  .tab:hover .close,
  .tab.selected .close {
    opacity: 1;
  }
  .close:hover {
    color: var(--fg);
    background: rgb(255 255 255 / 0.14);
  }
  /* 번호 배지는 고정 16x16 칸이라 탭 폭이 흔들리지 않는다. mac 의 ⌘번호는 글자만이라 바탕·모서리가 없다 */
  .badge {
    opacity: 1;
    font-size: 10px;
    font-weight: 600;
  }
</style>
