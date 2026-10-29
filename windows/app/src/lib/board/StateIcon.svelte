<script lang="ts">
  // 상태 표시(ClaudeBoard.swift StateIcon): 작업 중 도는 그라데이션 호(0.9초), 승인 대기 퍼지는 파문(1.1초),
  // 대기 통통 튀며 나타나는 체크, 일시중지 ⏸, 종료 빨간 ⊖. 상태가 바뀌면 새로 그려 등장 애니메이션을 다시 낸다
  import Icon from "./Icon.svelte";
  import { stateRgb } from "./format";

  let { state, scale = 1 }: { state: string; scale?: number } = $props();
</script>

<span class="box" style:--c={stateRgb(state)} style:--s={scale}>
  {#key state}
    <span class="inner">
      {#if state === "working"}
        <span class="track"></span>
        <span class="spin"><span class="head"></span></span>
      {:else if state === "waiting"}
        <span class="ripple"></span>
        <span class="dot"></span>
      {:else if state === "idle"}
        <span class="pop"><Icon name="check-circle" size={16} /></span>
      {:else if state === "paused"}
        <span class="pop"><Icon name="pause-circle" size={16} /></span>
      {:else}
        <span class="pop ended"><Icon name="minus-circle" size={15} /></span>
      {/if}
    </span>
  {/key}
</span>

<style>
  .box {
    position: relative;
    display: inline-block;
    flex: none;
    width: calc(18px * var(--s));
    height: calc(18px * var(--s));
    color: rgb(var(--c));
  }
  .inner {
    position: absolute;
    left: 0;
    top: 0;
    width: 18px;
    height: 18px;
    transform: scale(var(--s));
    transform-origin: top left;
    display: grid;
    place-items: center;
    animation: appear 0.25s ease-out;
  }
  /* SwiftUI 원은 선이 테두리 중앙에 놓여 18 칸 밖으로 1.25 씩 나온다 — 바깥 지름 20.5 */
  .track,
  .spin {
    position: absolute;
    inset: -1.25px;
    border-radius: 50%;
  }
  .track {
    border: 2.5px solid rgb(var(--c) / 0.18);
  }
  .spin {
    animation: spin 0.9s linear infinite;
  }
  /* 호 끝의 둥근 머리 — 호는 ::before 에 가리개로 그리므로 따로 둔다 */
  .head {
    position: absolute;
    width: 2.5px;
    height: 2.5px;
    left: calc(1.41px - 1.25px);
    top: calc(11.97px - 1.25px);
    border-radius: 50%;
    background: rgb(var(--c));
  }
  .spin::before {
    content: "";
    position: absolute;
    inset: 0;
    border-radius: 50%;
    background: conic-gradient(rgb(var(--c) / 0) 0deg, rgb(var(--c)) 259deg, transparent 259deg);
    mask: radial-gradient(farthest-side, transparent calc(100% - 2.6px), #000 calc(100% - 2.5px));
    -webkit-mask: radial-gradient(farthest-side, transparent calc(100% - 2.6px), #000 calc(100% - 2.5px));
  }
  .ripple {
    position: absolute;
    inset: 0;
    border-radius: 50%;
    background: rgb(var(--c) / 0.35);
    animation: ripple 1.1s cubic-bezier(0.25, 0.46, 0.45, 0.94) infinite;
  }
  .dot {
    width: 9px;
    height: 9px;
    border-radius: 50%;
    background: rgb(var(--c));
  }
  .pop {
    display: grid;
    place-items: center;
    animation: pop 0.45s cubic-bezier(0.34, 1.7, 0.64, 1);
  }
  .ended {
    color: rgb(237 84 84);
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  @keyframes ripple {
    from {
      transform: scale(0.8);
      opacity: 1;
    }
    to {
      transform: scale(1.9);
      opacity: 0;
    }
  }
  @keyframes pop {
    from {
      transform: scale(0.3);
      opacity: 0;
    }
    to {
      transform: scale(1);
      opacity: 1;
    }
  }
  @keyframes appear {
    from {
      opacity: 0;
    }
  }
</style>
