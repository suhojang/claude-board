<script lang="ts">
  // 세션 실행 방식 라벨: 터미널 창에서 도는지, 터미널 없이 백그라운드로 도는지, 서브에이전트인지.
  // compact 면 글자를 빼고 아이콘만
  import Icon from "./Icon.svelte";
  import { hint } from "./hint.svelte";

  let { headless = false, subagent = false, compact = false }: { headless?: boolean; subagent?: boolean; compact?: boolean } =
    $props();

  const c = $derived(subagent ? "242 140 191" : headless ? "184 140 255" : "82 199 219");
  const name = $derived(subagent ? "서브에이전트" : headless ? "백그라운드" : "터미널");
</script>

<span class="cap" class:compact style:--c={c} use:hint={name}>
  <Icon name={subagent ? "people" : headless ? "moon-stars" : "terminal"} size={subagent ? 12.6 : 10} pad={subagent ? 1.45 : 0} />
  {#if !compact}{name}{/if}
</span>

<style>
  .cap {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    flex: none;
    padding: 1.5px 6px;
    font-size: 10px;
    font-weight: 600;
    line-height: 13px;
    white-space: nowrap;
    color: rgb(var(--c));
    background: rgb(var(--c) / 0.15);
    box-shadow: inset 0 0 0 0.5px rgb(var(--c) / 0.25);
    border-radius: 999px;
  }
</style>
