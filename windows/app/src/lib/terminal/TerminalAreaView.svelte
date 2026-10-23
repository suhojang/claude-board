<script lang="ts">
  // 터미널 영역(ClaudeBoard.swift TerminalAreaView) — 위에서 아래로 제목 띠 · 경로 줄(탭이 있을 때만) · 탭 줄 · 화면 칸.
  // 탭이 없으면 화면 칸에 빈 상태를 보인다. 안 고른 탭도 마운트한 채 숨겨 셸과 출력이 그대로 남는다
  import { onTerminalFocus } from "$lib/backend";
  import { ui } from "$lib/board/store.svelte";
  import type { BoardSide } from "$lib/types";
  import NewSessionSheet from "./NewSessionSheet.svelte";
  import PaneView from "./PaneView.svelte";
  import TerminalWorkspaceBar from "./TerminalWorkspaceBar.svelte";
  import TerminalTabBar from "./TerminalTabBar.svelte";
  import TerminalEmptyView from "./TerminalEmptyView.svelte";
  import TerminalToggleButton from "./TerminalToggleButton.svelte";
  import WinButtons from "./WinButtons.svelte";
  import { TERM_BG } from "./term";
  import { beginRename, commitComposition, focusActive, TerminalHub, newSessionSheet, selectNumber, stepTab, stepWorkspace } from "./hub.svelte";
  import { confirmKeyAction, createRefireGuard } from "./keyGuards";

  let { expanded, side }: { expanded: boolean; side: BoardSide } = $props();

  $effect(() => onTerminalFocus(focusActive));

  // 수정키 — 누르는 즉시(다른 키와 함께여도) 번호 배지가 나타나고, 떼거나 창이 포커스를 잃으면 사라진다
  $effect(() => {
    const sync = (e: KeyboardEvent) => (TerminalHub.commandHeld = e.ctrlKey && e.altKey && !e.getModifierState("AltGraph"));
    const off = () => (TerminalHub.commandHeld = false);
    window.addEventListener("keydown", sync, true);
    window.addEventListener("keyup", sync, true);
    window.addEventListener("blur", off);
    return () => {
      window.removeEventListener("keydown", sync, true);
      window.removeEventListener("keyup", sync, true);
      window.removeEventListener("blur", off);
    };
  });

  // 터미널 단축키 — 펼친 동안만. xterm 이 받기 전에(캡처) 가로채 셸로 가지 않게 한다
  $effect(() => {
    if (!expanded) return;
    const refire = createRefireGuard();
    const handle = (e: KeyboardEvent) => {
      // Ctrl+Alt 단축키는 터미널 칸을 숨기거나 바꾼다. 조합 중에 칸이 숨겨지면 크롬이 조합을 취소하고
      // 입력기가 이후 키를 낱글자로 확정하는 상태에 빠지므로(약 1/5) 단축키가 오기 전에 조합 글자를 확정해 둔다
      if (e.ctrlKey && e.altKey && !e.getModifierState("AltGraph")) commitComposition();
      // 조합 중에 누른 단축키는 입력기가 같은 키를 한 번 더 쏜다 — 두 번째는 처리하지 않고 삼킨다(셸로도 보내지 않는다)
      if (refire.isRefire(e)) {
        e.preventDefault();
        e.stopPropagation();
        return;
      }
      if (TerminalHub.confirm || TerminalHub.sheet) return;
      // AltGr 은 Windows 가 Ctrl+Alt 로 보고한다 — 글자(@ { [ 등)를 치는 키라 가로채지 않는다
      if (e.getModifierState("AltGraph")) return;
      const run = (fn: () => void) => {
        e.preventDefault();
        e.stopPropagation();
        refire.handled(e);
        fn();
      };
      if (e.ctrlKey && e.altKey && !e.shiftKey) {
        if (/^Digit[1-9]$/.test(e.code)) return run(() => selectNumber(Number(e.code.slice(5))));
        if (e.code === "ArrowLeft") return run(() => stepTab(-1));
        if (e.code === "ArrowRight") return run(() => stepTab(1));
        if (e.code === "ArrowUp") return run(() => stepWorkspace(-1));
        if (e.code === "ArrowDown") return run(() => stepWorkspace(1));
      } else if (e.ctrlKey && !e.altKey && e.shiftKey && e.code === "KeyR") run(beginRename);
      else if (e.ctrlKey && !e.altKey && !e.shiftKey && e.code === "KeyT") run(newSessionSheet);
    };
    window.addEventListener("keydown", handle, true);
    return () => window.removeEventListener("keydown", handle, true);
  });

  // 확인 창의 [닫기] — 창을 지우기 전에 할 일을 꺼내 둔다
  function accept() {
    const run = TerminalHub.confirm?.run;
    TerminalHub.confirm = null;
    run?.();
  }

  // 확인 창 취소 — 포커스를 터미널로 돌려준다(안 돌리면 키 입력이 갈 데가 없다)
  function cancelConfirm() {
    TerminalHub.confirm = null;
    focusActive();
  }

  // 닫기 확인 창 — Esc 로 취소. 포커스가 터미널에 남아 있어 창 밖 키는 삼킨다(keyGuards.ts)
  function confirmKey(e: KeyboardEvent) {
    const insideDialog = e.target instanceof Element && e.target.closest("[role=alertdialog]") !== null;
    const action = confirmKeyAction(e.key, insideDialog);
    if (action === "pass") return;
    e.preventDefault();
    e.stopPropagation();
    if (action === "cancel") cancelConfirm();
  }
</script>

<!-- xterm 이 Esc 를 먼저 먹어 버블 단계에는 안 오므로 캡처로 받는다 -->
<svelte:window onkeydowncapture={TerminalHub.confirm ? confirmKey : undefined} />

<div class="area" style:--term-bg={TERM_BG}>
  <!-- 제목 띠 — 터미널 단추는 앞쪽 가장자리 x=8, 미니 단계에서는 숨긴다. 보드가 왼쪽이면 최소화·닫기가 이 띠 오른쪽 끝에 있다 -->
  <div class="band" data-tauri-drag-region>
    {#if ui.tier !== "mini"}
      <TerminalToggleButton expanded={true} {side} />
    {/if}
    <div class="drag" data-tauri-drag-region></div>
    {#if side === "left"}
      <WinButtons />
    {/if}
  </div>
  {#if TerminalHub.tabs.length}
    <TerminalWorkspaceBar />
  {/if}
  <TerminalTabBar />
  <div class="container">
    {#each TerminalHub.tabs as tab (tab.id)}
      <div class="surface" class:shown={tab.id === TerminalHub.selected}>
        <PaneView node={tab.root} tabId={tab.id} visible={expanded && tab.id === TerminalHub.selected} />
      </div>
    {/each}
    {#if TerminalHub.tabs.length === 0}
      <TerminalEmptyView />
    {/if}
  </div>

  {#if TerminalHub.sheet}
    <NewSessionSheet />
  {/if}

  {#if TerminalHub.confirm}
    {@const sheet = TerminalHub.confirm}
    <div class="scrim" role="presentation">
      <div class="sheet" role="alertdialog" aria-label={sheet.title}>
        <div class="title">{sheet.title}</div>
        <div class="text">{sheet.text}</div>
        <div class="buttons">
          <button class="primary" onclick={accept}>닫기</button>
          <button onclick={cancelConfirm}>취소 (Esc)</button>
        </div>
      </div>
    </div>
  {/if}
</div>

<style>
  .area {
    position: relative;
    width: 100%;
    height: 100%;
    display: flex;
    flex-direction: column;
    background: var(--term-bar);
  }
  .band {
    flex: none;
    height: 32px;
    display: flex;
    align-items: center;
    padding-left: 8px;
    background: var(--term-bar);
  }
  .drag {
    flex: 1;
    align-self: stretch;
  }
  .container {
    position: relative;
    flex: 1 1 0;
    min-height: 0;
    /* 엔진이 뜬 mac 보드는 화면 칸 바탕·고른 탭을 엔진의 바탕색(readBackground)으로 맞춘다. 같은 색이어야 고른 탭이 화면과 이어져 보인다 */
    background: var(--term-bg);
  }
  .surface {
    position: absolute;
    inset: 0;
    display: none;
  }
  .surface.shown {
    display: block;
  }
  .scrim {
    position: absolute;
    inset: 0;
    z-index: 30;
    display: grid;
    place-items: start center;
    padding-top: 90px;
    background: rgb(0 0 0 / 0.35);
  }
  .sheet {
    width: 320px;
    padding: 18px 18px 14px;
    border-radius: 12px;
    background: rgb(40 40 58 / 0.98);
    box-shadow:
      inset 0 0 0 0.5px rgb(255 255 255 / 0.16),
      0 12px 32px rgb(0 0 0 / 0.55);
  }
  .title {
    font-size: 13px;
    font-weight: 600;
  }
  .text {
    margin-top: 6px;
    font-size: 12px;
    color: var(--fg2);
  }
  .buttons {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 16px;
  }
  .buttons button {
    height: 26px;
    padding: 0 14px;
    border-radius: 7px;
    font-size: 12px;
    font-weight: 600;
    background: rgb(255 255 255 / 0.1);
  }
  .buttons button:hover {
    background: rgb(255 255 255 / 0.16);
  }
  .buttons button.primary {
    background: rgb(var(--accent) / 0.85);
    color: #fff;
  }
  .buttons button.primary:hover {
    background: rgb(var(--accent));
  }
</style>
