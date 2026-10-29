<script lang="ts">
  // 직접 그린 제목 막대(SPEC §5.10) — 끌기 영역, 왼쪽에 "처음 크기로 오른쪽에 붙이기", 오른쪽에 최대축소(↘↖)/원래 크기(⤢)·최소화·닫기.
  // 미니에서는 상태별 세션 수를 함께 보인다(자리가 모자라면 글자를 빼고 수만).
  // 터미널을 접은 동안은 snap 단추 오른쪽에 터미널 펼침 캡슐이 있다. 펼친 동안은 캡슐이 터미널 제목 띠에 있고,
  // 보드가 왼쪽이면 최소화·닫기도 터미널 제목 띠(창의 오른쪽 끝)로 간다
  import type { BoardSide, State } from "$lib/types";
  import { enterMini, leaveMini, snapRight } from "$lib/backend";
  import TerminalToggleButton from "$lib/terminal/TerminalToggleButton.svelte";
  import WinButtons from "$lib/terminal/WinButtons.svelte";
  import Fit from "./Fit.svelte";
  import Icon from "./Icon.svelte";
  import { stateName, stateRgb } from "./format";
  import { hint } from "./hint.svelte";
  import { keyLabel } from "$lib/shortcuts/store.svelte";

  let { mini, counts, rows, expanded, side }: { mini: boolean; counts: Record<State, number>; rows: number; expanded: boolean; side: BoardSide } = $props();

  const short: Record<string, string> = { waiting: "승인", working: "작업", idle: "대기" };
  const snapKey = $derived(keyLabel("snap"));
  const chips = $derived((["waiting", "working", "idle"] as const).filter((st) => counts[st] > 0));
</script>

<div class="bar" data-tauri-drag-region>
  <button class="snap" onclick={snapRight} use:hint={`처음 크기로 되돌려 화면 오른쪽에 붙이기${snapKey ? ` (${snapKey})` : ""}`}>
    <span class="cap"><Icon name="snap-right" size={11} /></span>
  </button>
  {#if !expanded && !mini}
    <TerminalToggleButton expanded={false} {side} />
  {/if}
  {#if mini}
    <Fit count={2} class="chips" data-tauri-drag-region>
      {#snippet content(level)}
        <span class="row">
          {#each chips as st (st)}
            <span class="chip num" style:--c={stateRgb(st)} use:hint={`${stateName(st)} ${counts[st]}개`}>
              <span class="dot"></span>{level === 0 ? `${counts[st]} ${short[st]}` : counts[st]}
            </span>
          {/each}
        </span>
      {/snippet}
    </Fit>
  {:else}
    <div class="drag" data-tauri-drag-region></div>
  {/if}
  <!-- 미니 = 원래 크기로, 전체·중간 = 최대축소(창을 가장 작게 줄여 경로마다 한 줄로 본다) -->
  <button
    class="tier"
    onclick={() => (mini ? leaveMini() : enterMini(rows))}
    use:hint={mini ? "원래 크기로 되돌리기" : "최대축소 — 창을 가장 작게 줄이고 경로마다 한 줄로 봅니다"}
  >
    <Icon name={mini ? "grow" : "shrink"} size={10} />
  </button>
  {#if !(expanded && side === "left")}
    <WinButtons round={!expanded} />
  {/if}
</div>

<style>
  .bar {
    display: flex;
    align-items: center;
    height: 32px;
    flex: none;
    padding-left: 4px;
    gap: 5px;
  }
  .drag {
    flex: 1;
    align-self: stretch;
  }
  .bar :global(.chips) {
    flex: 1;
    align-self: stretch;
    justify-content: flex-end;
  }
  .row {
    display: inline-flex;
    gap: 4px;
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    padding: 2px 6px;
    border-radius: 999px;
    font-size: 10.5px;
    font-weight: 700;
    white-space: nowrap;
    background: rgb(var(--c) / 0.15);
  }
  .dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: rgb(var(--c));
  }
  .snap {
    width: 34px;
    height: 28px;
    display: grid;
    place-items: center;
    flex: none;
  }
  .cap {
    width: 26px;
    height: 18px;
    display: grid;
    place-items: center;
    border-radius: 9px;
    color: rgb(255 255 255 / 0.8);
    background: rgb(255 255 255 / 0.18);
    box-shadow: inset 0 0 0 0.5px rgb(255 255 255 / 0.18);
    transition:
      transform 0.25s cubic-bezier(0.3, 1.6, 0.5, 1),
      background 0.15s;
  }
  .snap:hover .cap {
    color: #fff;
    background: rgb(255 255 255 / 0.3);
    transform: scale(1.12);
  }
  .tier {
    flex: none;
    width: 24px;
    height: 20px;
    margin-right: 2px;
    display: grid;
    place-items: center;
    border-radius: 10px;
    color: var(--fg2);
    background: rgb(255 255 255 / 0.08);
    transition: background 0.15s;
  }
  .tier:hover {
    color: var(--fg);
    background: rgb(255 255 255 / 0.16);
  }
</style>
