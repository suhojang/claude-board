<script lang="ts">
  // 탭 이름 입력 칸(ClaudeBoard.swift:6994-7034) — 이름 자리를 대신한다. 열릴 때 전체 선택, Enter 저장, Esc 취소, 포커스를 잃으면 저장(창이 포커스를 잃은 것은 제외).
  // 처음 값(탭 줄에 보이던 이름)을 고치지 않고 저장하면 허브가 아무 일도 하지 않는다
  import { untrack } from "svelte";
  import type { TerminalTab } from "$lib/types";
  import { endRename, shownName } from "./hub.svelte";

  let { tab }: { tab: TerminalTab } = $props();
  let input = $state<HTMLInputElement>();
  // 처음 값만 쓴다 — 입력 중에 탭 이름이 바뀌어도 입력 칸은 그대로
  let value = $state(untrack(() => shownName(tab)));

  $effect(() => {
    input?.focus();
    input?.select();
  });

  function blur() {
    if (document.hasFocus()) endRename(tab, value);
  }

  // Enter·Esc 와 blur 가 겹쳐 두 번 끝나지 않게 — 허브의 endRename 이 renaming 이 꺼진 뒤에는 무시한다
  function key(e: KeyboardEvent) {
    if (e.isComposing) return;
    if (e.key === "Enter") {
      e.preventDefault();
      endRename(tab, value);
    } else if (e.key === "Escape") {
      e.preventDefault();
      endRename(tab, null);
    }
    e.stopPropagation();
  }
</script>

<!-- 창이 포커스를 잃어서(다른 창이 앞으로 옴) 생긴 blur 는 이름 바꾸기를 끝내지 않는다 — 돌아오면 입력 칸이 그대로 포커스를 되찾는다 -->
<input
  bind:this={input}
  bind:value
  onkeydown={key}
  onblur={blur}
  spellcheck="false"
  autocomplete="off"
/>

<style>
  input {
    width: 120px;
    flex: none;
    padding: 0;
    margin: 0;
    border: none;
    outline: none;
    background: transparent;
    color: inherit;
    font: inherit;
    font-size: 13px;
    font-weight: 600;
    user-select: text;
    -webkit-user-select: text;
  }
</style>
