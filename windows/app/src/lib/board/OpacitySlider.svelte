<script lang="ts">
  // 오른쪽 아래 투명도 슬라이더 — 왼쪽으로 갈수록 투명하다. 끄는 동안과 놓은 뒤 1.2초는 고른 투명도를 미리 보여 준다
  import { setOpacity } from "$lib/backend";
  import Icon from "./Icon.svelte";
  import { hint } from "./hint.svelte";
  import { win } from "./pointer.svelte";

  let { value, compact = false }: { value: number; compact?: boolean } = $props();

  const level = $derived(win.dragging ? win.dragLevel : value);
  const p = $derived(((level - 0.3) / 0.7) * 100);
  let settle: ReturnType<typeof setTimeout> | undefined;

  function start() {
    clearTimeout(settle);
    win.dragLevel = value;
    win.dragging = true;
    win.previewing = true;
  }

  function end() {
    if (!win.dragging) return;
    setOpacity(Math.round(win.dragLevel * 100) / 100);
    win.dragging = false;
    // 놓은 뒤 잠깐 고른 투명도를 더 보여 주고 불투명으로 돌아간다(마우스가 보드 위에 있으므로)
    settle = setTimeout(() => (win.previewing = false), 1200);
  }
</script>

<div
  class="op"
  use:hint={"투명도 — 보드에서 마우스가 벗어나 있고 다른 앱을 쓰는 동안 이 투명도로 보입니다. 마우스를 올리거나 보드를 누르면 불투명해집니다"}
>
  <Icon name="half-circle" size={12} />
  <input
    type="range"
    min="0.3"
    max="1"
    step="0.01"
    value={level}
    style:width="{compact ? 48 : 84}px"
    style:--p="{p}%"
    onpointerdown={start}
    oninput={(e) => {
      if (!win.dragging) start();
      win.dragLevel = Number((e.currentTarget as HTMLInputElement).value);
    }}
    onpointerup={end}
    onchange={end}
    onkeyup={end}
    aria-label="투명도"
  />
  <span class="pct num">{Math.round(level * 100)}%</span>
</div>

<style>
  .op {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    color: var(--fg2);
    opacity: 0.6;
    transition: opacity 0.15s;
  }
  .op:hover {
    opacity: 1;
  }
  .pct {
    width: 32px;
    text-align: right;
    font-size: 10.8px;
    font-weight: 600;
  }
  input {
    -webkit-appearance: none;
    appearance: none;
    height: 14px;
    margin: 0;
    background: transparent;
    cursor: pointer;
  }
  input::-webkit-slider-runnable-track {
    height: 4px;
    border-radius: 2px;
    background: linear-gradient(90deg, rgb(var(--accent)) calc(var(--p) * 0.8095), rgb(255 255 255 / 0.18) calc(var(--p) * 0.8095) 68px, transparent 68px);
  }
  input::-webkit-slider-thumb {
    -webkit-appearance: none;
    width: 16px;
    height: 10px;
    margin-top: -3px;
    border-radius: 5px;
    background: transparent;
  }
  input::-moz-range-track {
    height: 4px;
    border-radius: 2px;
    background: linear-gradient(90deg, rgb(var(--accent)) calc(var(--p) * 0.8095), rgb(255 255 255 / 0.18) calc(var(--p) * 0.8095) 68px, transparent 68px);
  }
  input::-moz-range-thumb {
    width: 16px;
    height: 10px;
    border: none;
    border-radius: 5px;
    background: transparent;
  }
</style>
