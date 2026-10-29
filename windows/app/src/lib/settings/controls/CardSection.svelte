<script lang="ts">
  // 묶음 화면 안의 카드 묶음 — 위에 작은 제목, 아래에 줄 카드들(mac SettingsCard 의 제목 + 줄 목록)
  import type { Snippet } from "svelte";

  let { title, children }: { title: string; children: Snippet } = $props();
</script>

<section class="section">
  <h3>{title}</h3>
  <div class="rows">{@render children()}</div>
</section>

<style>
  .section {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 4px 24px 12px;
  }
  h3 {
    margin: 0 0 0 4px;
    font-size: 11.5px;
    font-weight: 600;
    color: var(--fg2);
  }
  .rows {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
</style>
