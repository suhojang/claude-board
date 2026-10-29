<script lang="ts">
  // 캐릭터 고르기 — 여덟 캐릭터가 제자리에서 움직인다. 고른 캐릭터는 강조 테두리(mac MascotGrid)
  import { clock } from "$lib/mascot/clock.svelte";
  import Figure from "$lib/mascot/Figure.svelte";
  import { MASCOTS } from "$lib/mascot/mascots";
  import type { MascotKind } from "$lib/types";

  let { selected, onpick }: { selected: MascotKind; onpick: (kind: MascotKind) => void } = $props();
</script>

<div class="grid" role="radiogroup" aria-label="캐릭터">
  {#each MASCOTS as m (m.kind)}
    {@const on = m.kind === selected}
    <button type="button" role="radio" aria-checked={on} class="tile" class:on data-mascot={m.kind} onclick={() => onpick(m.kind)}>
      <Figure kind={m.kind} mood={on ? "busy" : "idle"} t={clock.t} sinceChange={9} blink={false} />
      <span class="name">{m.name}</span>
    </button>
  {/each}
</div>

<style>
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(104px, 1fr));
    gap: 8px;
  }
  .tile {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    padding: 10px 4px 6px;
    border-radius: 10px;
    background: rgb(255 255 255 / 0.025);
    box-shadow: inset 0 0 0 1px rgb(255 255 255 / 0.07);
    transition: background 0.15s, box-shadow 0.15s;
  }
  .tile:hover:not(.on) {
    background: rgb(255 255 255 / 0.06);
  }
  .tile.on {
    background: rgb(var(--accent) / 0.14);
    box-shadow: inset 0 0 0 1.5px rgb(var(--accent) / 0.8);
  }
  .name {
    min-height: 28px;
    font-size: 10.5px;
    line-height: 1.3;
    text-align: center;
    color: var(--fg2);
  }
  .on .name {
    color: var(--fg);
    font-weight: 600;
  }
</style>
