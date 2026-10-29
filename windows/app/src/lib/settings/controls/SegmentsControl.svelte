<script lang="ts" generics="Value extends string">
  // 설정 줄 오른쪽의 고르기 단추 묶음(mac SettingsSegments) — 하나만 고른다
  let { options, selected, label, onpick }: { options: { value: Value; text: string }[]; selected: Value; label: string; onpick: (value: Value) => void } = $props();
</script>

<div class="segments" role="radiogroup" aria-label={label}>
  {#each options as option (option.value)}
    <button type="button" role="radio" aria-checked={option.value === selected} class:on={option.value === selected} onclick={() => onpick(option.value)}>
      {option.text}
    </button>
  {/each}
</div>

<style>
  .segments {
    display: inline-flex;
    padding: 2px;
    border-radius: 8px;
    background: rgb(255 255 255 / 0.07);
  }
  button {
    padding: 4px 11px;
    border-radius: 6px;
    font-size: 12px;
    color: var(--fg2);
    transition: background 0.15s, color 0.15s;
  }
  button:hover:not(.on) {
    color: var(--fg);
  }
  button.on {
    background: rgb(255 255 255 / 0.16);
    color: var(--fg);
    font-weight: 600;
  }
</style>
