<script lang="ts">
  // 화면 그림에서 알림 위치를 고르는 칸 — 그림 속 자리(작은 사각형)를 누르거나, 그림의 아무 데나 눌러 가장 가까운 자리를 고른다.
  // 규칙(가장 가까운 자리)은 screenPick.ts 가 정한다(mac NotifyPositionPicker)
  import type { NotifyPosition } from "$lib/types";
  import { POSITION_NAMES, SPOTS, SPOT_ORDER, pickPosition } from "./screenPick";

  let { value, onpick }: { value: NotifyPosition; onpick: (position: NotifyPosition) => void } = $props();

  // 화면 비율에 가깝게(mac 은 132×82). 자리는 그림 크기에 비례해 놓인다
  const WIDTH = 176;
  const HEIGHT = 110;

  function pickAt(event: MouseEvent) {
    if (event.target !== event.currentTarget) return; // 자리 단추는 자기가 고른다
    const box = (event.currentTarget as HTMLElement).getBoundingClientRect();
    onpick(pickPosition({ width: box.width, height: box.height }, { x: event.clientX - box.left, y: event.clientY - box.top }));
  }
</script>

<!-- 키보드는 자리 단추가 받는다 — 그림 바탕 누르기는 마우스 덤이다 -->
<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="picker" data-testid="screen-picker" style:width="{WIDTH}px" style:height="{HEIGHT}px" onclick={pickAt}>
  <div class="bar"></div>
  {#each SPOT_ORDER as position (position)}
    <button
      type="button"
      class="spot"
      class:on={position === value}
      style:left="{SPOTS[position].x * 100}%"
      style:top="{SPOTS[position].y * 100}%"
      aria-label={POSITION_NAMES[position]}
      aria-pressed={position === value}
      data-position={position}
      onclick={() => onpick(position)}
    ></button>
  {/each}
</div>

<style>
  .picker {
    position: relative;
    flex: none;
    border-radius: 9px;
    background: rgb(0 0 0 / 0.3);
    box-shadow: inset 0 0 0 1px rgb(255 255 255 / 0.12);
    overflow: hidden;
    cursor: pointer;
  }
  .bar {
    position: absolute;
    inset: 0 0 auto;
    height: 7px;
    background: rgb(255 255 255 / 0.08);
    pointer-events: none;
  }
  .spot {
    position: absolute;
    width: 40px;
    height: 19px;
    transform: translate(-50%, -50%);
    border-radius: 4px;
    background: rgb(255 255 255 / 0.16);
    transition: background 0.15s, box-shadow 0.15s;
  }
  .spot:hover:not(.on) {
    background: rgb(255 255 255 / 0.32);
  }
  .spot.on {
    background: rgb(var(--accent));
    box-shadow: 0 0 6px rgb(var(--accent) / 0.6);
  }
</style>
