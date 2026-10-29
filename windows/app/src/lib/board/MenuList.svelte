<script lang="ts">
  import Self from "./MenuList.svelte";
  import { hint } from "./hint.svelte";
  import type { MenuItem } from "./menu.svelte";

  // anchor.left 가 있으면 하위 메뉴 — 오른쪽 자리가 모자라면 부모 항목 왼쪽에, 양쪽 다 모자라면(좁은 창) 항목 아래에 연다
  let {
    items,
    anchor,
    onDone,
  }: { items: MenuItem[]; anchor: { x: number; y: number; left?: number; below?: number }; onDone: () => void } =
    $props();

  let w = $state(0);
  let h = $state(0);
  let sub = $state<{ index: number; x: number; y: number; left: number; below: number } | null>(null);

  const pos = $derived.by(() => {
    let x = anchor.x;
    let y = anchor.y;
    if (x + w > window.innerWidth - 4) {
      if (anchor.left != null && anchor.left - w >= 4) x = anchor.left - w;
      else {
        x = window.innerWidth - w - 4;
        if (anchor.below != null) y = anchor.below;
      }
    }
    if (y + h > window.innerHeight - 4) y = window.innerHeight - h - 4;
    return { x: Math.max(4, x), y: Math.max(4, y) };
  });

  function enter(i: number, e: MouseEvent) {
    const it = items[i];
    if ("separator" in it || !it.submenu || it.enabled === false) {
      sub = null;
      return;
    }
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    sub = { index: i, x: r.right - 2, y: r.top - 5, left: r.left + 2, below: r.bottom - 3 };
  }

  function pick(it: MenuItem) {
    if ("separator" in it || it.header || it.enabled === false || it.submenu) return;
    onDone();
    it.action?.();
  }
</script>

<div class="menu" role="menu" tabindex="-1" style:left="{pos.x}px" style:top="{pos.y}px" bind:offsetWidth={w} bind:offsetHeight={h}>
  {#each items as it, i (i)}
    {#if "separator" in it}
      <div class="sep"></div>
    {:else if it.header}
      <div class="header">{it.label}</div>
    {:else}
      <button
        class="item"
        class:off={it.enabled === false}
        class:destructive={it.destructive}
        class:open={sub?.index === i}
        role="menuitem"
        disabled={it.enabled === false}
        onmouseenter={(e) => enter(i, e)}
        onclick={() => pick(it)}
        use:hint={it.title ?? ""}
      >
        <span class="check">{it.checked ? "✓" : ""}</span>
        <span class="label">{it.label}</span>
        {#if it.keys}<span class="keys">{it.keys}</span>{/if}
        {#if it.submenu}<span class="arrow">›</span>{/if}
      </button>
    {/if}
  {/each}
</div>

{#if sub}
  {@const it = items[sub.index]}
  {#if !("separator" in it) && it.submenu}
    <Self items={it.submenu} anchor={{ x: sub.x, y: sub.y, left: sub.left, below: sub.below }} {onDone} />
  {/if}
{/if}

<style>
  .menu {
    position: fixed;
    z-index: 900;
    min-width: 180px;
    max-width: min(420px, calc(100vw - 8px));
    padding: 5px;
    background: rgb(36 36 48 / 0.98);
    border: 0.5px solid rgb(255 255 255 / 0.16);
    border-radius: 8px;
    box-shadow: 0 10px 28px rgb(0 0 0 / 0.5);
    font-size: 12.5px;
    color: var(--fg);
    outline: none;
  }
  .item {
    display: flex;
    align-items: center;
    width: 100%;
    gap: 2px;
    padding: 4px 10px 4px 4px;
    border-radius: 5px;
    text-align: left;
    white-space: nowrap;
  }
  .item:hover:not(.off),
  .item.open {
    background: rgb(var(--accent) / 0.85);
    color: #fff;
  }
  .item.off {
    color: rgb(255 255 255 / 0.32);
  }
  .item.destructive:not(:hover) {
    color: rgb(255 120 120);
  }
  .check {
    width: 18px;
    flex: none;
    text-align: center;
    font-size: 11px;
  }
  .label {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .keys {
    margin-left: 18px;
    font-size: 11.5px;
    opacity: 0.6;
  }
  .arrow {
    margin-left: 14px;
    font-size: 14px;
    line-height: 1;
    opacity: 0.7;
  }
  .header {
    padding: 4px 10px 4px 22px;
    font-size: 11.5px;
    color: rgb(255 255 255 / 0.4);
    white-space: nowrap;
  }
  .sep {
    height: 1px;
    margin: 4px 6px;
    background: rgb(255 255 255 / 0.1);
  }
</style>
